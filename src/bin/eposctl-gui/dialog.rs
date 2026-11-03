//! Native open/save dialogs for JSON files.

use std::path::PathBuf;

use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
    OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

/// Ask for a `.json` file to save to (`save`) or open. Blocks until the dialog closes.
pub fn json_file(save: bool, default_name: &str) -> Option<PathBuf> {
    // Pairs of (description, pattern), each NUL-terminated, then a final NUL.
    let filter: Vec<u16> = "JSON (*.json)\0*.json\0\0".encode_utf16().collect();
    let ext: Vec<u16> = "json\0".encode_utf16().collect();
    let mut buf = vec![0u16; 1024];
    if save {
        for (d, c) in buf.iter_mut().zip(default_name.encode_utf16().take(1000)) {
            *d = c;
        }
    }
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = unsafe { GetActiveWindow() };
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrDefExt = ext.as_ptr();
    ofn.Flags = OFN_EXPLORER
        | OFN_NOCHANGEDIR
        | OFN_PATHMUSTEXIST
        | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } };
    if ok == 0 {
        return None; // cancelled (or failed)
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
}
