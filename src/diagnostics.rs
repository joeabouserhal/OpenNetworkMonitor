//! A small log in the data folder, and what the app does when it crashes.
//!
//! The log records what the app does at start and exit, opening the history, starting the
//! capture, merging old history and anything that fails, so a problem can be traced afterwards. It
//! never holds traffic data. A crash handler writes the details of a panic or a Windows exception
//! to the same log and saves the traffic that was still waiting in memory. It is best effort: a
//! crash can leave the process in a state where nothing works, but this is better than nothing.
use std::{
    cell::Cell,
    collections::VecDeque,
    fmt,
    fs::{File, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const LOG_NAME: &str = "onm.log";
/// The log is rotated when it would pass this size, keeping one older copy.
const MAX_LOG_BYTES: u64 = 1 << 20;
/// How many of the latest lines a crash report repeats.
const RECENT_LINES: usize = 40;
const SESSION_NAME: &str = "session.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}
impl Level {
    fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
        }
    }
}

/// An open log file.
struct Log {
    dir: PathBuf,
    file: File,
    size: u64,
    recent: VecDeque<String>,
}
impl Log {
    fn open(dir: &Path) -> std::io::Result<Self> {
        let path = dir.join(LOG_NAME);
        let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            file,
            size,
            recent: VecDeque::new(),
        })
    }
    /// Adds one line, starting a new file first if this one is full. Each line is written out at
    /// once, so a crash right afterwards does not lose it.
    fn write(&mut self, level: Level, message: &str) {
        let line = format!(
            "{} {} {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            level.label(),
            message
        );
        if self.size + line.len() as u64 > MAX_LOG_BYTES {
            self.rotate();
        }
        if self.file.write_all(line.as_bytes()).is_ok() {
            let _ = self.file.flush();
            self.size += line.len() as u64;
        }
        self.recent.push_back(line.trim_end().to_owned());
        while self.recent.len() > RECENT_LINES {
            self.recent.pop_front();
        }
    }
    fn rotate(&mut self) {
        let current = self.dir.join(LOG_NAME);
        let older = self.dir.join(format!("{LOG_NAME}.1"));
        // The older copy is replaced; if the file cannot be moved it is started over instead.
        let _ = std::fs::remove_file(&older);
        let _ = std::fs::rename(&current, &older);
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&current)
        {
            self.file = file;
            self.size = 0;
        }
    }
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);
fn with_log<T>(use_log: impl FnOnce(&mut Log) -> T) -> Option<T> {
    let mut guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.as_mut().map(use_log)
}

/// Starts logging to `onm.log` in `dir`. Until this is called, logging does nothing.
pub fn init(dir: &Path) {
    if let Ok(log) = Log::open(dir) {
        *LOG.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(log);
    }
}
/// The log file, once logging has started.
pub fn log_path() -> Option<PathBuf> {
    with_log(|log| log.dir.join(LOG_NAME))
}
pub fn write(level: Level, message: fmt::Arguments<'_>) {
    let text = message.to_string();
    with_log(|log| log.write(level, &text));
}
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::diagnostics::write($crate::diagnostics::Level::Info, format_args!($($arg)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::diagnostics::write($crate::diagnostics::Level::Warn, format_args!($($arg)*)) };
}
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::diagnostics::write($crate::diagnostics::Level::Error, format_args!($($arg)*)) };
}

// ------------------------------------------------------------------------------------ sessions

/// How the run before this one ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviousSession {
    /// It closed normally, or there was none.
    Clean,
    /// It was still marked as running and its process is gone: a crash, a force close or a power
    /// cut. `crash` says what the crash handler recorded, when it got to.
    Unclean {
        started: String,
        version: String,
        crash: Option<String>,
    },
}

static SESSION_FILE: OnceLock<PathBuf> = OnceLock::new();
static PREVIOUS: OnceLock<PreviousSession> = OnceLock::new();

fn read_marker(path: &Path) -> Option<std::collections::HashMap<String, String>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect(),
    )
}

/// When the process `pid` was created and whether it is still running, or `None` if there is no
/// such process. A process that cannot be queried is assumed to be running.
fn process_state(pid: u32) -> Option<(u64, bool)> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ACCESS_DENIED, FILETIME, GetLastError, STILL_ACTIVE},
        System::Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    };
    // SAFETY: asks Windows about a process with query-only rights; the handle is closed below.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return (GetLastError() == ERROR_ACCESS_DENIED).then_some((0, true));
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut ended, mut kernel, mut user) = (zero, zero, zero, zero);
        GetProcessTimes(handle, &mut created, &mut ended, &mut kernel, &mut user);
        let mut code = 0u32;
        GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        Some((created, code as i32 == STILL_ACTIVE))
    }
}

/// What the marker at `path` says about the run that left it.
fn previous_from(path: &Path) -> PreviousSession {
    let Some(marker) = read_marker(path) else {
        return PreviousSession::Clean;
    };
    let pid: u32 = marker.get("pid").and_then(|p| p.parse().ok()).unwrap_or(0);
    // Still running means another copy, such as a newer build taking over: not a crash. A process
    // with the same number but another creation time is a different program that reused it.
    let marked_creation: Option<u64> = marker.get("created").and_then(|c| c.parse().ok());
    let still_running = pid == std::process::id()
        || process_state(pid).is_some_and(|(created, running)| {
            running && (created == 0 || marked_creation.is_none_or(|marked| marked == created))
        });
    if still_running {
        return PreviousSession::Clean;
    }
    PreviousSession::Unclean {
        started: marker.get("started").cloned().unwrap_or_default(),
        version: marker.get("version").cloned().unwrap_or_default(),
        crash: marker.get("crash").cloned(),
    }
}

/// Marks this run as started and reports how the last one ended. The marker is removed by a
/// normal exit, so a marker left behind by a process that no longer exists means a crash.
pub fn begin_session(dir: &Path, version: &str) -> PreviousSession {
    let path = dir.join(SESSION_NAME);
    let previous = previous_from(&path);
    let _ = std::fs::write(
        &path,
        format!(
            "pid={}\ncreated={}\nstarted={}\nversion={version}\n",
            std::process::id(),
            process_state(std::process::id()).map_or(0, |(created, _)| created),
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        ),
    );
    let _ = SESSION_FILE.set(path);
    let _ = PREVIOUS.set(previous.clone());
    previous
}
/// How the previous run ended, as found by `begin_session`.
pub fn previous_session() -> PreviousSession {
    PREVIOUS.get().cloned().unwrap_or(PreviousSession::Clean)
}
/// Marks this run as finished normally.
pub fn end_session() {
    if let Some(path) = SESSION_FILE.get()
        && read_marker(path)
            .and_then(|marker| marker.get("pid").and_then(|pid| pid.parse::<u32>().ok()))
            == Some(std::process::id())
    {
        let _ = std::fs::remove_file(path);
    }
}
/// Records in the session marker what the crash was, for the next launch to report.
fn note_crash(summary: &str) {
    if let Some(path) = SESSION_FILE.get()
        && let Ok(mut file) = OpenOptions::new().append(true).open(path)
    {
        let one_line = summary.replace(['\r', '\n'], " ");
        let _ = writeln!(file, "crash={one_line}");
    }
}

// ------------------------------------------------------------------------------------ crashes

type Emergency = Box<dyn Fn() -> String + Send + Sync>;
static EMERGENCY: OnceLock<Emergency> = OnceLock::new();

/// Sets what a crash does to save the traffic still waiting in memory. It returns a sentence
/// saying what happened, for the log.
pub fn set_emergency_save(save: impl Fn() -> String + Send + Sync + 'static) {
    let _ = EMERGENCY.set(Box::new(save));
}
fn emergency_save() -> String {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EMERGENCY
            .get()
            .map_or_else(|| "no emergency save was set up".to_owned(), |save| save())
    }))
    .unwrap_or_else(|_| "the emergency save itself failed".to_owned())
}

thread_local! {
    /// Set while this thread is reporting a crash, so a failure in the report cannot start another.
    static REPORTING: Cell<bool> = const { Cell::new(false) };
}

/// What a panic message says, whatever type the panic carried.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned())
}

/// Logs a crash and saves what is waiting. `what` is a one-line description.
fn report_crash(what: &str, details: &str) {
    if REPORTING.with(|reporting| reporting.replace(true)) {
        return;
    }
    crate::log_error!("CRASH: {what}");
    if !details.is_empty() {
        crate::log_error!("CRASH: details:\n{details}");
    }
    let recent = with_log(|log| log.recent.iter().cloned().collect::<Vec<_>>().join("\n"));
    if let Some(recent) = recent {
        crate::log_error!(
            "CRASH: version {}, process {}, the log before it:\n{recent}",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        );
    }
    note_crash(what);
    let outcome = emergency_save();
    crate::log_error!("CRASH: emergency save: {outcome}");
    REPORTING.with(|reporting| reporting.set(false));
}

/// Starts recording panics and Windows exceptions in the log, and saving what is waiting.
pub fn install_crash_handlers() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let place = info.location().map_or_else(
            || "an unknown place".to_owned(),
            |at| format!("{}:{}:{}", at.file(), at.line(), at.column()),
        );
        let what = format!(
            "panic on thread '{}' at {place}: {}",
            thread.name().unwrap_or("unnamed"),
            panic_message(info.payload())
        );
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        report_crash(&what, &backtrace);
        previous(info);
    }));
    // SAFETY: registers a function that only logs and saves; Windows calls it with a pointer to
    // the exception it is reporting.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(Some(
            exception_filter,
        ));
    }
}

/// # Safety
/// Called only by Windows, with the exception that is about to end the process.
unsafe extern "system" fn exception_filter(
    info: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    // SAFETY: Windows passes a valid pointer, or null, to its own exception records.
    let (code, address) = unsafe {
        info.as_ref()
            .and_then(|info| info.ExceptionRecord.as_ref())
            .map_or((0, 0), |record| {
                (record.ExceptionCode, record.ExceptionAddress as usize)
            })
    };
    report_crash(
        &format!("unhandled Windows exception 0x{code:08x} at address 0x{address:x}"),
        "",
    );
    // Let Windows carry on with its own handling, which closes the program.
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_carry_a_time_and_a_level_and_survive_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Log::open(dir.path()).unwrap();
        log.write(Level::Info, "starting");
        log.write(Level::Error, "something failed");
        let text = std::fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("INFO  starting"), "{}", lines[0]);
        assert!(lines[1].ends_with("ERROR something failed"), "{}", lines[1]);
        // "2026-11-10 07:41:27.123 " is a date, a time to the millisecond and a space.
        assert_eq!(lines[0].find("INFO").unwrap(), 24);
        // Opening the log again adds to it rather than starting over.
        let mut again = Log::open(dir.path()).unwrap();
        again.write(Level::Warn, "back");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(LOG_NAME))
                .unwrap()
                .lines()
                .count(),
            3
        );
    }

    #[test]
    fn a_full_log_keeps_one_older_copy_and_starts_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Log::open(dir.path()).unwrap();
        let filler = "x".repeat(1000);
        for _ in 0..1200 {
            log.write(Level::Info, &filler);
        }
        log.write(Level::Info, "after the rotation");
        let current = std::fs::metadata(dir.path().join(LOG_NAME)).unwrap().len();
        let older = std::fs::metadata(dir.path().join("onm.log.1"))
            .unwrap()
            .len();
        assert!(current < MAX_LOG_BYTES && older <= MAX_LOG_BYTES);
        assert!(older > MAX_LOG_BYTES / 2, "the full file was kept");
        assert!(
            std::fs::read_to_string(dir.path().join(LOG_NAME))
                .unwrap()
                .contains("after the rotation")
        );
        assert!(
            !dir.path().join("onm.log.2").exists(),
            "only one older copy"
        );
    }

    #[test]
    fn a_crash_report_can_repeat_the_lines_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Log::open(dir.path()).unwrap();
        for n in 0..100 {
            log.write(Level::Info, &format!("step {n}"));
        }
        assert_eq!(log.recent.len(), RECENT_LINES);
        assert!(log.recent.back().unwrap().ends_with("step 99"));
        assert!(log.recent.front().unwrap().ends_with("step 60"));
    }

    #[test]
    fn panic_messages_read_the_same_whatever_they_carry() {
        assert_eq!(panic_message(&"plain"), "plain");
        assert_eq!(panic_message(&String::from("formatted 3")), "formatted 3");
        assert_eq!(panic_message(&42_u8), "(no message)");
    }

    #[test]
    fn a_run_that_left_its_marker_behind_is_reported_as_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SESSION_NAME);
        // No marker: the last run closed normally, or there was none.
        assert_eq!(previous_from(&path), PreviousSession::Clean);
        // A marker from a process that no longer exists, with what the crash handler noted.
        std::fs::write(
            &path,
            "pid=4294967290
started=2026-11-10 07:41:27
version=0.5.2
crash=panic at x.rs:1
",
        )
        .unwrap();
        assert_eq!(
            previous_from(&path),
            PreviousSession::Unclean {
                started: "2026-11-10 07:41:27".into(),
                version: "0.5.2".into(),
                crash: Some("panic at x.rs:1".into()),
            }
        );
        // A marker from a process that is still alive is a newer build taking over, not a crash.
        std::fs::write(
            &path,
            format!(
                "pid={}
started=now
version=1
",
                std::process::id()
            ),
        )
        .unwrap();
        assert_eq!(previous_from(&path), PreviousSession::Clean);
        // This process is described as running, with a creation time; an impossible one is not.
        let (created, running) = process_state(std::process::id()).unwrap();
        assert!(created > 0 && running);
        assert_eq!(process_state(u32::MAX - 3), None);
        // Without a crash note, an unclean end is still reported.
        std::fs::write(
            &path,
            "pid=4294967290
started=t
version=v
",
        )
        .unwrap();
        assert!(matches!(
            previous_from(&path),
            PreviousSession::Unclean { crash: None, .. }
        ));
    }

    /// The part that crashes. It runs in a second copy of this test program, started by
    /// `a_real_crash_is_logged_and_the_waiting_traffic_saved`, and never returns.
    fn crash_in_a_child(scenario: &str) -> ! {
        use crate::{model::UsageRow, store::Store};
        use windows_sys::Win32::System::Diagnostics::Debug::{
            SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
        };
        let dir = PathBuf::from(std::env::var("ONM_CRASH_DIR").unwrap());
        init(&dir);
        install_crash_handlers();
        begin_session(&dir, "crash-test");
        crate::log_info!("the child starts");
        let mut store = Store::open(&dir.join("usage.db")).unwrap();
        store.stage(
            &std::collections::BTreeMap::from([(
                600,
                vec![UsageRow {
                    source: "transport".into(),
                    adapter_id: "eth".into(),
                    app: "a.exe".into(),
                    remote: "1.1.1.1".into(),
                    protocol: "TCP".into(),
                    scope: "WAN".into(),
                    down: 100,
                    up: 10,
                    ..Default::default()
                }],
            )]),
            &[],
        );
        let handle = store.crash_handle().unwrap();
        set_emergency_save(move || match handle.save() {
            Ok(rows) => format!("saved {rows} waiting rows"),
            Err(e) => format!("could not save: {e}"),
        });
        // Without this the child would wait for someone to close Windows' crash dialog.
        // SAFETY: only changes how this process reports its own crashes.
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
        // The store is not dropped: only the crash handler may save what is waiting.
        std::mem::forget(store);
        if scenario == "panic" {
            panic!("boom for the test");
        }
        // SAFETY: deliberately writes through a null pointer to crash this child with a Windows
        // access violation.
        unsafe { std::ptr::null_mut::<u8>().write_volatile(1) };
        unreachable!("the write above faults");
    }

    #[test]
    fn a_real_crash_is_logged_and_the_waiting_traffic_saved() {
        if let Ok(scenario) = std::env::var("ONM_CRASH_SCENARIO") {
            crash_in_a_child(&scenario);
        }
        for (scenario, expected) in [
            ("panic", "panic on thread"),
            ("fault", "unhandled Windows exception 0xc0000005"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "diagnostics::tests::a_real_crash_is_logged_and_the_waiting_traffic_saved",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("ONM_CRASH_SCENARIO", scenario)
                .env("ONM_CRASH_DIR", dir.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(!status.success(), "{scenario}: the child really crashed");
            // The log says what happened and what the emergency save did.
            let log = std::fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
            assert!(log.contains("the child starts"), "{scenario}: {log}");
            assert!(
                log.contains(&format!("CRASH: {expected}")) || log.contains(expected),
                "{scenario}: {log}"
            );
            assert!(
                log.contains("emergency save: saved 1 waiting rows"),
                "{scenario}: {log}"
            );
            if scenario == "panic" {
                assert!(log.contains("boom for the test") && log.contains("diagnostics.rs"));
            }
            // The next launch can tell that the run crashed, and why.
            match previous_from(&dir.path().join(SESSION_NAME)) {
                PreviousSession::Unclean { version, crash, .. } => {
                    assert_eq!(version, "crash-test");
                    assert!(
                        crash.is_some_and(|crash| crash.contains(expected)),
                        "{scenario}"
                    );
                }
                other => panic!("{scenario}: expected an unclean session, got {other:?}"),
            }
            // And the traffic that was only in memory is in the history.
            let store = crate::store::Store::open(&dir.path().join("usage.db")).unwrap();
            let filter = crate::model::Filter {
                source: crate::model::Source::Transport,
                time_range: Some(crate::model::TimeRange {
                    start: 600,
                    end: 700,
                }),
                ..Default::default()
            };
            assert_eq!(store.history(&filter, 700).unwrap().down, 100, "{scenario}");
        }
    }
}
