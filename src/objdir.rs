//! Enumerate named kernel objects in `\BaseNamedObjects` (the `Global\` namespace).
//!
//! The EPOS APO settings channel is named after the USB VID/PID of the device it
//! serves, so we discover available devices by listing section objects instead of
//! guessing PIDs.

use std::ffi::c_void;
use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::HANDLE;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *const UnicodeString,
    attributes: u32,
    security_descriptor: *const c_void,
    security_quality_of_service: *const c_void,
}

#[repr(C)]
struct ObjectDirectoryInformation {
    name: UnicodeString,
    type_name: UnicodeString,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenDirectoryObject(handle: *mut HANDLE, access: u32, attrs: *const ObjectAttributes) -> i32;
    fn NtQueryDirectoryObject(
        handle: HANDLE,
        buffer: *mut c_void,
        length: u32,
        return_single_entry: u8,
        restart_scan: u8,
        context: *mut u32,
        return_length: *mut u32,
    ) -> i32;
    fn NtClose(handle: HANDLE) -> i32;
}

const DIRECTORY_QUERY: u32 = 0x0001;
const OBJ_CASE_INSENSITIVE: u32 = 0x0040;

/// One entry of an object directory.
#[derive(Debug, Clone)]
pub struct ObjectEntry {
    pub type_name: String,
    pub name: String,
}

unsafe fn ustr_to_string(s: &UnicodeString) -> String {
    if s.buffer.is_null() || s.length == 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(s.buffer, (s.length / 2) as usize) };
    String::from_utf16_lossy(slice)
}

/// List every object in the given object-manager directory, e.g. `\BaseNamedObjects`.
pub fn list(directory: &str) -> io::Result<Vec<ObjectEntry>> {
    let mut wide: Vec<u16> = directory.encode_utf16().collect();
    let name = UnicodeString {
        length: (wide.len() * 2) as u16,
        maximum_length: (wide.len() * 2) as u16,
        buffer: wide.as_mut_ptr(),
    };
    let attrs = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: ptr::null_mut(),
        object_name: &name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null(),
        security_quality_of_service: ptr::null(),
    };

    let mut handle: HANDLE = ptr::null_mut();
    let status = unsafe { NtOpenDirectoryObject(&mut handle, DIRECTORY_QUERY, &attrs) };
    if status != 0 {
        return Err(io::Error::other(format!(
            "NtOpenDirectoryObject({directory}) failed: NTSTATUS {status:#010x}"
        )));
    }

    // u64 backing store keeps the buffer suitably aligned for the pointer-bearing structs.
    let mut buf = vec![0u64; 8192];
    let mut out = Vec::new();
    let mut context = 0u32;
    loop {
        let mut returned = 0u32;
        let status = unsafe {
            NtQueryDirectoryObject(
                handle,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                1,
                0,
                &mut context,
                &mut returned,
            )
        };
        if status != 0 {
            break; // STATUS_NO_MORE_ENTRIES or an error: either way we are done
        }
        let info = unsafe { &*(buf.as_ptr() as *const ObjectDirectoryInformation) };
        out.push(ObjectEntry {
            type_name: unsafe { ustr_to_string(&info.type_name) },
            name: unsafe { ustr_to_string(&info.name) },
        });
    }
    unsafe { NtClose(handle) };
    Ok(out)
}
