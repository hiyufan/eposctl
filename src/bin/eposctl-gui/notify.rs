//! Windows toast notifications for an unpackaged app.
//!
//! Toasts need an AppUserModelID. Registering one under
//! `HKCU\Software\Classes\AppUserModelId\<id>` with a display name is enough
//! for `ToastNotificationManager` to accept it; no Start menu shortcut needed.

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

const AUMID: &str = "eposctl.gui";

fn register() {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    let key: Vec<u16> = format!(r"Software\Classes\AppUserModelId\{AUMID}").encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = "DisplayName".encode_utf16().chain(Some(0)).collect();
    let value: Vec<u16> = "eposctl".encode_utf16().chain(Some(0)).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        );
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Show a toast with a title and one line of text. Errors are returned, not shown.
pub fn toast(title: &str, body: &str) -> windows::core::Result<()> {
    unsafe {
        // Fine if the thread is already initialised (eframe uses STA on the UI thread).
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_SINGLETHREADED);
    }
    register();
    let xml = XmlDocument::new()?;
    xml.LoadXml(&HSTRING::from(format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(title),
        escape(body)
    )))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
}
