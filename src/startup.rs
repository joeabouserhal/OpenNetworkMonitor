//! Opt-in, per-user startup using an interactive, elevated logon task.
use anyhow::{Context, Result, bail, ensure};
use std::{path::Path, sync::mpsc};
use windows::{
    Win32::System::{
        Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        TaskScheduler::{
            IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TASK_CREATE_OR_UPDATE,
            TASK_LOGON_INTERACTIVE_TOKEN, TaskScheduler,
        },
        Variant::VARIANT,
    },
    core::{BSTR, Interface},
};

const MARKER: &str = "OpenNetworkMonitor: per-user startup (managed by the app)";
const PREFIX: &str = "OpenNetworkMonitor Startup - ";

#[derive(Default)]
pub struct Status {
    pub enabled: bool,
    pub different_copy: bool,
}

/// UI state is refreshed from Windows, rather than trusting a saved checkbox.
pub struct Controller {
    tx: Option<mpsc::Sender<bool>>,
    rx: Option<mpsc::Receiver<Result<Status>>>,
    pub enabled: bool,
    pub busy: bool,
    pub different_copy: bool,
    pub error: String,
    pub available: bool,
}
impl Controller {
    pub fn new(ctx: eframe::egui::Context) -> Self {
        let (tx, commands) = mpsc::channel();
        let (results, rx) = mpsc::channel();
        let spawn = std::thread::Builder::new()
            .name("startup-settings".into())
            .spawn(move || {
                let Ok(exe) = std::env::current_exe() else {
                    let _ = results.send(Err(anyhow::anyhow!(
                        "Could not locate this copy of the app."
                    )));
                    ctx.request_repaint();
                    return;
                };
                let _ = results.send(status(&exe));
                ctx.request_repaint();
                while let Ok(enabled) = commands.recv() {
                    let result = set_enabled(enabled, &exe).and_then(|()| status(&exe));
                    let _ = results.send(result);
                    ctx.request_repaint();
                }
            });
        let mut controller = Self::preview();
        match spawn {
            Ok(_) => {
                controller.tx = Some(tx);
                controller.rx = Some(rx);
                controller.busy = true;
                controller.available = true;
            }
            Err(e) => controller.error = format!("Startup settings unavailable: {e}"),
        }
        controller
    }
    /// No Windows calls or startup changes in previews and UI tests.
    pub fn preview() -> Self {
        Self {
            tx: None,
            rx: None,
            enabled: false,
            busy: false,
            different_copy: false,
            error: String::new(),
            available: false,
        }
    }
    pub fn poll(&mut self) {
        if let Some(rx) = &self.rx {
            for result in rx.try_iter() {
                self.busy = false;
                match result {
                    Ok(status) => {
                        self.enabled = status.enabled;
                        self.different_copy = status.different_copy;
                        self.error.clear();
                    }
                    Err(error) => self.error = format!("Could not update startup: {error:#}"),
                }
            }
        }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        if let Some(tx) = &self.tx {
            if tx.send(enabled).is_ok() {
                self.busy = true;
                self.error.clear();
            } else {
                self.error = "Startup settings are unavailable. Reopen the app to retry.".into();
            }
        }
    }
}

struct Apartment(bool);
impl Apartment {
    fn enter() -> Result<Self> {
        // SAFETY: Initializes COM on this thread; the guard balances successful calls.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self(true))
        } else if result.0 == 0x80010106_u32 as i32 {
            Ok(Self(false))
        } else {
            Err(windows::core::Error::from_hresult(result).into())
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: This thread successfully initialized COM in enter().
            unsafe { CoUninitialize() };
        }
    }
}
fn with_scheduler<T>(action: impl FnOnce(&ITaskFolder, &str) -> Result<T>) -> Result<T> {
    let _apartment = Apartment::enter()?;
    let sid = current_sid()?;
    // SAFETY: COM is initialized, interfaces own their references and all BSTR/VARIANT
    // arguments remain valid for these synchronous local service calls.
    let service: ITaskService =
        unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER) }?;
    let empty = VARIANT::default();
    // SAFETY: Empty credentials connect to the local service as the current user.
    unsafe { service.Connect(&empty, &empty, &empty, &empty) }?;
    // SAFETY: Valid initialized service and an owned, terminated BSTR path.
    let folder = unsafe { service.GetFolder(&BSTR::from("\\")) }?;
    action(&folder, &sid)
}
fn task_name(sid: &str) -> BSTR {
    BSTR::from(format!("{PREFIX}{sid}"))
}
fn owned_task(folder: &ITaskFolder, sid: &str) -> Result<Option<IRegisteredTask>> {
    // SAFETY: Valid COM folder and owned BSTR. Missing tasks are the default off state.
    let task = match unsafe { folder.GetTask(&task_name(sid)) } {
        Ok(task) => task,
        Err(error) if error.code().0 == 0x80070002_u32 as i32 => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut description = BSTR::new();
    // SAFETY: Returned COM objects and the BSTR output pointer are valid and owned.
    unsafe {
        task.Definition()?
            .RegistrationInfo()?
            .Description(&mut description)
    }?;
    ensure!(
        description == MARKER,
        "A different task uses the startup name; it was left unchanged."
    );
    Ok(Some(task))
}
fn status(exe: &Path) -> Result<Status> {
    with_scheduler(|folder, sid| {
        let Some(task) = owned_task(folder, sid)? else {
            return Ok(Status::default());
        };
        let mut path = BSTR::new();
        let mut arguments = BSTR::new();
        // SAFETY: Task-owned interfaces are valid, Item(1) selects the first action,
        // and cast checks that it is an executable action before reading its fields.
        let enabled = unsafe {
            let action: IExecAction = task.Definition()?.Actions()?.get_Item(1)?.cast()?;
            action.Path(&mut path)?;
            action.Arguments(&mut arguments)?;
            task.Enabled()?.as_bool()
        };
        Ok(Status {
            enabled,
            different_copy: !same_path(Path::new(&path.to_string()), exe)
                || arguments != "--startup",
        })
    })
}
fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    };
    normalize(a) == normalize(b)
}
fn set_enabled(enabled: bool, exe: &Path) -> Result<()> {
    ensure!(
        exe.is_absolute() && exe.is_file(),
        "The current executable is unavailable."
    );
    with_scheduler(|folder, sid| {
        let existing = owned_task(folder, sid)?;
        if !enabled {
            if existing.is_some() {
                // SAFETY: Only the verified app-owned task for this user's SID is deleted.
                unsafe { folder.DeleteTask(&task_name(sid), 0) }?;
            }
            return Ok(());
        }
        let xml = task_xml(sid, exe)?;
        let empty = VARIANT::default();
        // SAFETY: The XML names the current user's SID, uses an interactive token,
        // and contains only this executable's action. No passwords are stored.
        unsafe {
            folder.RegisterTask(
                &task_name(sid),
                &BSTR::from(xml),
                TASK_CREATE_OR_UPDATE.0,
                &VARIANT::from(sid),
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                &empty,
            )
        }?;
        Ok(())
    })
}
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn task_xml(sid: &str, exe: &Path) -> Result<String> {
    ensure!(
        exe.is_absolute(),
        "Startup requires an absolute executable path."
    );
    let path = exe
        .to_str()
        .context("The executable path cannot be represented in task XML")?;
    let parent = exe
        .parent()
        .context("The executable has no parent directory")?
        .to_str()
        .context("Invalid working directory")?;
    Ok(format!(
        r#"<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<RegistrationInfo><Description>{MARKER}</Description></RegistrationInfo>
<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{sid}</UserId></LogonTrigger></Triggers>
<Principals><Principal id="User"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>false</AllowHardTerminate><StartWhenAvailable>true</StartWhenAvailable><RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable><Enabled>true</Enabled><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings>
<Actions Context="User"><Exec><Command>{}</Command><Arguments>--startup</Arguments><WorkingDirectory>{}</WorkingDirectory></Exec></Actions>
</Task>"#,
        xml_escape(path),
        xml_escape(parent)
    ))
}
fn current_sid() -> Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // SAFETY: Query-only token access, aligned API-sized storage, checked API
    // results and null-terminated SID output. Every owned handle/allocation is freed.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut size = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
        if size < std::mem::size_of::<TOKEN_USER>() as u32 {
            CloseHandle(token);
            bail!("Could not read the current user.");
        }
        let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        let success = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        );
        let error = std::io::Error::last_os_error();
        CloseHandle(token);
        if success == 0 {
            return Err(error.into());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let length = (0..184).find(|&i| *text.add(i) == 0).unwrap_or(184);
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        ensure!(length < 184, "Invalid user SID string.");
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_task_targets_one_user_and_escapes_executable_paths() -> Result<()> {
        let xml = task_xml(
            "S-1-5-21-123",
            Path::new(r"C:\Apps & Tools\OpenNetworkMonitor.exe"),
        )?;
        assert!(xml.contains("<UserId>S-1-5-21-123</UserId>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains(r"<Command>C:\Apps &amp; Tools\OpenNetworkMonitor.exe</Command>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(same_path(
            Path::new(r"\\?\C:\Apps\Monitor.exe"),
            Path::new(r"c:\apps\monitor.exe")
        ));
        assert!(task_xml("sid", Path::new("relative.exe")).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "Reads local startup state and validates task XML in memory; does not register a task"]
    fn native_startup_definition_is_valid_without_registration() -> Result<()> {
        let _apartment = Apartment::enter()?;
        let sid = current_sid()?;
        let exe = std::env::current_exe()?;
        // SAFETY: COM is initialized; no task is registered or executed by NewTask/SetXmlText.
        unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
            let task = service.NewTask(0)?;
            task.SetXmlText(&BSTR::from(task_xml(&sid, &exe)?))?;
        }
        let _ = status(&exe)?;
        Ok(())
    }
}
