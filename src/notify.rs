//! Windows toast notifications for alerts.
//!
//! An unpackaged app needs an app identity before Windows shows its toasts. It is
//! registered for the current user only, as a name under their own registry hive.
use crate::alerts::Notice;
use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::HSTRING,
};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegSetValueExW,
};

const APP_ID: &str = "OpenNetworkMonitor.App";
const APP_NAME: &str = "OpenNetworkMonitor";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Makes Windows know the app by name so its toasts are attributed and shown.
fn register_identity() -> bool {
    let key_path = wide(&format!(r"Software\Classes\AppUserModelId\{APP_ID}"));
    let value_name = wide("DisplayName");
    let value = wide(APP_NAME);
    let mut key = std::ptr::null_mut();
    // SAFETY: the names are null-terminated and outlive the calls; the key handle is
    // closed before returning, and the value bytes cover exactly the UTF-16 string.
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            return false;
        }
        let written = RegSetValueExW(
            key,
            value_name.as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        );
        RegCloseKey(key);
        written == 0
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn toast_xml(notice: &Notice) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        xml_escape(&notice.title),
        xml_escape(&notice.body)
    )
}

struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances this thread's successful RoInitialize.
            unsafe { RoUninitialize() };
        }
    }
}

/// Shows one toast. Returns false when Windows refused or toasts are unavailable.
pub fn show(notice: &Notice) -> bool {
    // SAFETY: initializes WinRT on this thread; the guard balances a success. A thread
    // that already has a different apartment is still able to use the API.
    let apartment = Apartment(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok());
    let shown = (|| -> windows::core::Result<()> {
        if !register_identity() {
            return Err(windows::core::Error::from_thread());
        }
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(toast_xml(notice)))?;
        let toast = ToastNotification::CreateToastNotification(&document)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&toast)
    })()
    .is_ok();
    drop(apartment);
    shown
}

/// Shows a toast without holding up the caller. If it cannot be shown, `fallback`
/// gets the notice so it can be put somewhere the user will still see it.
pub fn deliver(notice: Notice, fallback: impl FnOnce(&Notice) + Send + 'static) {
    let thread = std::thread::Builder::new()
        .name("toast".into())
        .spawn(move || {
            if !show(&notice) {
                fallback(&notice);
            }
        });
    let _ = thread;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_text_is_escaped_so_app_names_cannot_break_the_markup() {
        let xml = toast_xml(&Notice {
            title: "New <app> & \"more\"".into(),
            body: "O'Reilly".into(),
        });
        assert!(xml.contains("New &lt;app&gt; &amp; &quot;more&quot;"));
        assert!(xml.contains("O&apos;Reilly"));
        assert!(!xml.contains("<app>"));
    }

    #[test]
    fn the_markup_loads_as_a_toast_document() {
        // SAFETY: initializes WinRT for this test thread only.
        let _apartment = Apartment(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok());
        let document = XmlDocument::new().expect("xml document");
        document
            .LoadXml(&HSTRING::from(toast_xml(&Notice {
                title: "Data cap reached".into(),
                body: "You have used 100 GiB of 100 GiB & more".into(),
            })))
            .expect("valid markup");
    }
}
