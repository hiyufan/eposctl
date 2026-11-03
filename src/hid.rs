//! Minimal Win32 HID access for the EPOS dongle's vendor collection.

use std::io;
use std::mem::{size_of, zeroed};
use std::ptr;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidD_GetProductString, HidD_SetNumInputBuffers, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile,
    WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows_sys::core::GUID;

pub const EPOS_VID: u16 = 0x1395;
pub const VENDOR_USAGE_PAGE: u16 = 0xFFFF;

#[derive(Debug, Clone)]
pub struct HidInfo {
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub input_len: u16,
    pub output_len: u16,
    /// USB product string, if the device reports one.
    pub product: Option<String>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn open_path(path: &str, access: u32) -> HANDLE {
    unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        )
    }
}

unsafe fn product_string(h: HANDLE) -> Option<String> {
    let mut buf = [0u16; 128];
    if unsafe { HidD_GetProductString(h, buf.as_mut_ptr().cast(), (buf.len() * 2) as u32) } == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Query attributes and top-level caps of one HID interface (opened with no access rights).
fn probe(path: &str) -> Option<HidInfo> {
    unsafe {
        let h = open_path(path, 0);
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut attrs: HIDD_ATTRIBUTES = zeroed();
        attrs.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        let mut info = None;
        if HidD_GetAttributes(h, &mut attrs) != 0 {
            let mut pp = 0;
            if HidD_GetPreparsedData(h, &mut pp) != 0 {
                let mut caps: HIDP_CAPS = zeroed();
                if HidP_GetCaps(pp, &mut caps) >= 0 {
                    info = Some(HidInfo {
                        path: path.to_string(),
                        vid: attrs.VendorID,
                        pid: attrs.ProductID,
                        usage_page: caps.UsagePage,
                        usage: caps.Usage,
                        input_len: caps.InputReportByteLength,
                        output_len: caps.OutputReportByteLength,
                        product: product_string(h),
                    });
                }
                HidD_FreePreparsedData(pp);
            }
        }
        CloseHandle(h);
        info
    }
}

/// Enumerate all present HID interfaces.
pub fn enumerate() -> Vec<HidInfo> {
    let mut out = Vec::new();
    unsafe {
        let mut guid: GUID = zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set as isize == -1 {
            return out;
        }
        let mut i = 0;
        loop {
            let mut ifd: SP_DEVICE_INTERFACE_DATA = zeroed();
            ifd.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(set, ptr::null(), &guid, i, &mut ifd) == 0 {
                break;
            }
            i += 1;
            let mut need = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &ifd, ptr::null_mut(), 0, &mut need, ptr::null_mut());
            if need == 0 {
                continue;
            }
            let mut buf = vec![0u64; (need as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, detail, need, ptr::null_mut(), ptr::null_mut()) == 0 {
                continue;
            }
            let p = ptr::addr_of!((*detail).DevicePath) as *const u16;
            let len = (0..).take_while(|&k| *p.add(k) != 0).count();
            let path = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            if let Some(info) = probe(&path) {
                out.push(info);
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// Find EPOS vendor command collections (usage page 0xFFFF).
pub fn find_vendor_collections() -> Vec<HidInfo> {
    enumerate()
        .into_iter()
        .filter(|d| d.vid == EPOS_VID && d.usage_page == VENDOR_USAGE_PAGE)
        .collect()
}

pub struct HidDevice {
    handle: HANDLE,
    event: HANDLE,
    pub info: HidInfo,
}

unsafe impl Send for HidDevice {}

impl HidDevice {
    /// Open for reading only (passive monitoring; never interferes with writes).
    pub fn open_read(info: &HidInfo) -> io::Result<Self> {
        Self::open(info, GENERIC_READ)
    }

    /// Open for reading and writing.
    pub fn open_rw(info: &HidInfo) -> io::Result<Self> {
        Self::open(info, GENERIC_READ | GENERIC_WRITE)
    }

    fn open(info: &HidInfo, access: u32) -> io::Result<Self> {
        unsafe {
            let handle = open_path(&info.path, access);
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            HidD_SetNumInputBuffers(handle, 256);
            let event = CreateEventW(ptr::null(), 1, 0, ptr::null());
            if event.is_null() {
                let e = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(e);
            }
            Ok(Self { handle, event, info: info.clone() })
        }
    }

    /// Read one input report. Returns `Ok(None)` on timeout.
    pub fn read(&self, timeout_ms: u32) -> io::Result<Option<Vec<u8>>> {
        let len = self.info.input_len.max(1) as usize;
        let mut buf = vec![0u8; len];
        unsafe {
            let mut ov: OVERLAPPED = zeroed();
            ov.hEvent = self.event;
            let mut got = 0u32;
            if ReadFile(self.handle, buf.as_mut_ptr(), len as u32, &mut got, &mut ov) == 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    return Err(err);
                }
                if WaitForSingleObject(self.event, timeout_ms) != WAIT_OBJECT_0 {
                    CancelIoEx(self.handle, &ov);
                    GetOverlappedResult(self.handle, &ov, &mut got, 1);
                    return Ok(None);
                }
                if GetOverlappedResult(self.handle, &ov, &mut got, 0) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            buf.truncate(got as usize);
            Ok(Some(buf))
        }
    }

    /// Write one full output report (first byte = report id).
    pub fn write(&self, report: &[u8]) -> io::Result<()> {
        unsafe {
            let mut ov: OVERLAPPED = zeroed();
            ov.hEvent = self.event;
            let mut done = 0u32;
            if WriteFile(self.handle, report.as_ptr(), report.len() as u32, &mut done, &mut ov) == 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    return Err(err);
                }
                if GetOverlappedResult(self.handle, &ov, &mut done, 1) == 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
            CloseHandle(self.event);
        }
    }
}
