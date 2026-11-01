//! Single-instance lock and hand-over between builds.
//!
//! A running copy holds a named mutex, records its build stamp in the data
//! folder and opens a named event. A newer build that finds the lock taken
//! signals that event, waits for the old copy to finish its normal exit (final
//! writes included) and then takes over. Older or identical builds never
//! replace a running copy, and sign-in launches never replace anything.
use super::windows::OwnedHandle;
use anyhow::{Context, Result, bail};
use std::{
    path::Path,
    ptr, thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0},
    Storage::FileSystem::SYNCHRONIZE,
    System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, ResetEvent, SetEvent,
        WaitForSingleObject,
    },
};

const BUILD_STAMP_FILE: &str = "running-build.txt";

#[derive(Clone)]
struct Names {
    lock: Vec<u16>,
    quit: Vec<u16>,
}
impl Names {
    fn app() -> Self {
        Self::with_prefix("OpenNetworkMonitor")
    }
    fn with_prefix(prefix: &str) -> Self {
        let wide = |suffix: &str| -> Vec<u16> {
            format!("Local\\{prefix}.{suffix}\0")
                .encode_utf16()
                .collect()
        };
        Self {
            lock: wide("Collector.v1"),
            quit: wide("Quit.v1"),
        }
    }
}

struct Waits {
    /// How long to look for the running copy's quit event, which it opens a moment after starting.
    find_quit: Duration,
    /// How long the running copy gets to save its data and exit once asked.
    exit: Duration,
}
impl Waits {
    const NORMAL: Self = Self {
        find_quit: Duration::from_secs(3),
        exit: Duration::from_secs(20),
    };
}

pub struct InstanceGuard {
    _lock: OwnedHandle,
    quit: Option<OwnedHandle>,
    names: Names,
}
impl InstanceGuard {
    /// For launches that must never replace a running copy, such as sign-in startup.
    pub fn try_acquire() -> Result<Option<Self>> {
        Self::try_acquire_named(Names::app())
    }
    /// Takes the lock, first asking an older running build to exit if there is one.
    pub fn acquire_or_replace(data_dir: &Path) -> Result<Self> {
        Self::replace_named(Names::app(), data_dir, build_stamp(), &Waits::NORMAL)
    }
    /// Records this build for later launches and opens the event that asks it to exit.
    pub fn publish(&mut self, data_dir: &Path) {
        self.publish_stamp(data_dir, build_stamp());
    }

    fn try_acquire_named(names: Names) -> Result<Option<Self>> {
        // SAFETY: null security attributes and a terminated name are accepted by CreateMutexW.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, names.lock.as_ptr()) };
        if handle.is_null() {
            bail!("Cannot create application lock");
        }
        let lock = OwnedHandle(handle);
        // SAFETY: GetLastError reads the last API result for this thread.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Ok(None);
        }
        Ok(Some(Self {
            _lock: lock,
            quit: None,
            names,
        }))
    }
    fn publish_stamp(&mut self, data_dir: &Path, stamp: u64) {
        // Best effort: without the stamp a later launch simply won't try to replace this copy.
        let _ = std::fs::write(data_dir.join(BUILD_STAMP_FILE), stamp.to_string());
        // SAFETY: null attributes, manual reset, initially unsignaled, terminated name.
        let event = unsafe { CreateEventW(ptr::null(), 1, 0, self.names.quit.as_ptr()) };
        if event.is_null() {
            return;
        }
        // We hold the lock, so no other copy is running: a signal on an event that
        // already existed is left over from the copy we replaced and must not quit us.
        // SAFETY: `event` is a valid event handle created above.
        unsafe { ResetEvent(event) };
        self.quit = Some(OwnedHandle(event));
    }
    fn replace_named(names: Names, data_dir: &Path, own: u64, waits: &Waits) -> Result<Self> {
        if let Some(guard) = Self::try_acquire_named(names.clone())? {
            return Ok(guard);
        }
        let running = std::fs::read_to_string(data_dir.join(BUILD_STAMP_FILE))
            .ok()
            .and_then(|text| text.trim().parse::<u64>().ok());
        if !should_replace(running, own) {
            bail!("OpenNetworkMonitor is already running. Open it from its system tray icon.");
        }
        request_quit(&names, waits.find_quit).context(
            "An older OpenNetworkMonitor is running and cannot be replaced automatically. \
             Exit it from its tray icon (right-click, then Exit) and open this one again.",
        )?;
        let deadline = Instant::now() + waits.exit;
        loop {
            thread::sleep(Duration::from_millis(100));
            if let Some(guard) = Self::try_acquire_named(names.clone())? {
                return Ok(guard);
            }
            if Instant::now() >= deadline {
                bail!(
                    "The running OpenNetworkMonitor did not exit in time. \
                     Exit it from its tray icon and open this one again."
                );
            }
        }
    }
}

/// Runs `on_quit` once, on a background thread, if a newer build asks this copy to exit.
pub fn spawn_quit_listener(on_quit: impl FnOnce() + Send + 'static) {
    listen(Names::app(), on_quit);
}

fn listen(names: Names, on_quit: impl FnOnce() + Send + 'static) {
    thread::spawn(move || {
        // SAFETY: a terminated name; SYNCHRONIZE is all that waiting needs.
        let event = unsafe { OpenEventW(SYNCHRONIZE, 0, names.quit.as_ptr()) };
        if event.is_null() {
            return;
        }
        let event = OwnedHandle(event);
        // SAFETY: the guard keeps the handle valid for the duration of the wait.
        if unsafe { WaitForSingleObject(event.0, INFINITE) } == WAIT_OBJECT_0 {
            on_quit();
        }
    });
}

fn request_quit(names: &Names, find: Duration) -> Result<()> {
    let deadline = Instant::now() + find;
    loop {
        // SAFETY: a terminated name; EVENT_MODIFY_STATE is all that signaling needs.
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, names.quit.as_ptr()) };
        if !event.is_null() {
            let event = OwnedHandle(event);
            // SAFETY: the guard keeps the handle valid for the call. It is closed on
            // return, before this process creates an event with the same name.
            if unsafe { SetEvent(event.0) } != 0 {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            bail!("The running copy has no quit event");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Only a strictly newer build replaces a running one. An unknown stamp comes from a copy
/// that may predate this mechanism, so it is still asked, and the attempt fails quickly.
fn should_replace(running: Option<u64>, own: u64) -> bool {
    running.is_none_or(|running| own > running)
}

/// Seconds since the epoch at which this executable was last written, so a fresh build
/// outranks the copy it replaces.
fn build_stamp() -> u64 {
    std::env::current_exe()
        .and_then(|path| path.metadata())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    fn names(tag: &str) -> Names {
        let owner =
            std::env::var("ONM_TEST_OWNER").unwrap_or_else(|_| std::process::id().to_string());
        Names::with_prefix(&format!("OpenNetworkMonitorTest.{owner}.{tag}"))
    }
    const QUICK: Waits = Waits {
        find_quit: Duration::from_millis(300),
        exit: Duration::from_secs(10),
    };

    #[test]
    fn only_a_strictly_newer_or_unknown_build_replaces_a_running_one() {
        assert!(should_replace(Some(100), 101));
        assert!(!should_replace(Some(100), 100));
        assert!(!should_replace(Some(100), 99));
        assert!(should_replace(None, 1));
        assert!(
            build_stamp() > 0,
            "The test executable has a modification time"
        );
    }

    #[test]
    fn a_newer_build_asks_the_running_one_to_exit_and_takes_over() {
        let dir = tempfile::tempdir().unwrap();
        let mut running = InstanceGuard::try_acquire_named(names("takeover"))
            .unwrap()
            .unwrap();
        running.publish_stamp(dir.path(), 100);
        let (asked, was_asked) = mpsc::channel();
        listen(names("takeover"), move || {
            let _ = asked.send(());
        });
        let path = dir.path().to_path_buf();
        let newer = thread::spawn(move || {
            let guard = InstanceGuard::replace_named(names("takeover"), &path, 200, &QUICK)
                .map_err(|e| e.to_string())?;
            let mut guard = guard;
            guard.publish_stamp(&path, 200);
            // A signal left over from the copy that was replaced must not quit this one.
            let quit = Arc::new(AtomicBool::new(false));
            let flag = quit.clone();
            listen(names("takeover"), move || {
                flag.store(true, Ordering::SeqCst)
            });
            thread::sleep(Duration::from_millis(400));
            Ok::<bool, String>(quit.load(Ordering::SeqCst))
        });
        was_asked
            .recv_timeout(Duration::from_secs(5))
            .expect("The running copy was never asked to exit");
        // The running copy finishes its normal exit, which releases the lock.
        drop(running);
        let quit_itself = newer
            .join()
            .unwrap()
            .expect("The newer build did not take over");
        assert!(
            !quit_itself,
            "The new copy must not inherit the old quit signal"
        );
    }

    #[test]
    fn an_older_or_identical_build_never_replaces_a_running_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut running = InstanceGuard::try_acquire_named(names("keep"))
            .unwrap()
            .unwrap();
        running.publish_stamp(dir.path(), 100);
        let (asked, was_asked) = mpsc::channel();
        listen(names("keep"), move || {
            let _ = asked.send(());
        });
        for own in [100, 50] {
            let error = InstanceGuard::replace_named(names("keep"), dir.path(), own, &QUICK)
                .err()
                .expect("An older or identical build must not take over")
                .to_string();
            assert!(error.contains("already running"), "{error}");
        }
        assert!(
            was_asked.recv_timeout(Duration::from_millis(300)).is_err(),
            "The running copy must not be asked to exit"
        );
    }

    #[test]
    fn a_copy_that_cannot_be_asked_fails_quickly_with_instructions() {
        let dir = tempfile::tempdir().unwrap();
        // Holds the lock but never opens a quit event, like a build from before this feature.
        let _legacy = InstanceGuard::try_acquire_named(names("legacy"))
            .unwrap()
            .unwrap();
        let started = Instant::now();
        let error = InstanceGuard::replace_named(names("legacy"), dir.path(), 200, &QUICK)
            .err()
            .expect("There is nothing to hand over from")
            .to_string();
        assert!(
            error.contains("cannot be replaced automatically"),
            "{error}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    /// Plays the running app when launched by the test below; does nothing otherwise.
    #[test]
    fn separate_process_holder() {
        let Ok(dir) = std::env::var("ONM_TEST_DIR") else {
            return;
        };
        let mut running = InstanceGuard::try_acquire_named(names("process"))
            .unwrap()
            .unwrap();
        running.publish_stamp(Path::new(&dir), 100);
        // Like the real app, leave through process exit, which is what releases the lock.
        listen(names("process"), || std::process::exit(0));
        println!("READY");
        thread::sleep(Duration::from_secs(30));
        std::process::exit(3);
    }

    #[test]
    fn a_newer_build_replaces_a_separately_running_process() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::instance::tests::separate_process_holder",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ONM_TEST_DIR", dir.path())
            .env("ONM_TEST_OWNER", std::process::id().to_string())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        assert!(
            // libtest prints the test name before the test's own output on the same line.
            lines.any(|line| line.is_ok_and(|line| line.ends_with("READY"))),
            "The stand-in app never started"
        );
        let replaced = InstanceGuard::replace_named(names("process"), dir.path(), 200, &QUICK);
        let status = child.wait().unwrap();
        assert!(
            replaced.is_ok(),
            "{:?}",
            replaced.err().map(|e| e.to_string())
        );
        assert!(
            status.success(),
            "The old process should exit when asked: {status}"
        );
    }
}
