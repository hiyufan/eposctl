//! Windows audio devices: list them, read the default, and change the default.
//!
//! Changing the default device uses `IPolicyConfig`, an undocumented COM interface
//! that Windows' own sound settings use (as do EarTrumpet and SoundSwitch). It has
//! been stable since Windows 7.

use std::ffi::c_void;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
    eCommunications, eConsole, eMultimedia, eRender,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::core::{GUID, HRESULT, HSTRING, IUnknown, Interface, PCWSTR, PWSTR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// Playback (speakers, headphones).
    Output,
    /// Recording (microphones).
    Input,
}

impl Flow {
    fn raw(self) -> EDataFlow {
        match self {
            Flow::Output => eRender,
            Flow::Input => eCapture,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Stable endpoint id (`{0.0.0.00000000}.{guid}`), the same across reboots.
    pub id: String,
    /// Name as shown in Windows sound settings, e.g. `Speakers (EPOS GSA 70)`.
    pub name: String,
}

fn com() {
    // Fine if this thread already joined an apartment (eframe's UI thread is STA).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    com();
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

/// Take ownership of a COM-allocated string.
unsafe fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

fn describe(dev: &IMMDevice) -> windows::core::Result<Endpoint> {
    unsafe {
        let id = take(dev.GetId()?);
        let props = dev.OpenPropertyStore(STGM_READ)?;
        let value = props.GetValue(&PKEY_Device_FriendlyName)?;
        let name = take(PropVariantToStringAlloc(&value)?);
        Ok(Endpoint { id, name })
    }
}

/// Active devices of one direction, in Windows' order.
pub fn endpoints(flow: Flow) -> windows::core::Result<Vec<Endpoint>> {
    let e = enumerator()?;
    unsafe {
        let list = e.EnumAudioEndpoints(flow.raw(), DEVICE_STATE_ACTIVE)?;
        let mut out = Vec::new();
        for i in 0..list.GetCount()? {
            if let Ok(ep) = describe(&list.Item(i)?) {
                out.push(ep);
            }
        }
        Ok(out)
    }
}

/// Id of the current default device (the "console" role Windows shows as default).
pub fn default_id(flow: Flow) -> windows::core::Result<Option<String>> {
    let e = enumerator()?;
    match unsafe { e.GetDefaultAudioEndpoint(flow.raw(), eConsole) } {
        Ok(dev) => Ok(Some(unsafe { take(dev.GetId()?) })),
        Err(_) => Ok(None), // no device of this kind
    }
}

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
/// `IPolicyConfig::SetDefaultEndpoint`: 3 `IUnknown` slots, then 10 methods before it.
const SET_DEFAULT_ENDPOINT_SLOT: usize = 13;

/// Make `id` the default device for every role (default device, default
/// communication device), like choosing it in Windows sound settings.
pub fn set_default(id: &str) -> windows::core::Result<()> {
    com();
    unsafe {
        let unk: IUnknown = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
        let mut raw: *mut c_void = std::ptr::null_mut();
        unk.query(&IID_POLICY_CONFIG, &mut raw).ok()?;
        let pc = IUnknown::from_raw(raw); // released on drop
        type SetDefault = unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT;
        let vtbl = *(pc.as_raw() as *const *const *const c_void);
        let f: SetDefault = std::mem::transmute(*vtbl.add(SET_DEFAULT_ENDPOINT_SLOT));
        let id = HSTRING::from(id);
        for role in [eConsole, eMultimedia, eCommunications] {
            f(pc.as_raw(), PCWSTR(id.as_ptr()), role).ok()?;
        }
        Ok(())
    }
}

/// Whether a device name belongs to an EPOS headset or dongle.
pub fn is_epos(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("epos") || n.contains("sennheiser")
}

/// Find a device by id, or by a case-insensitive part of its name. Errors list
/// the candidates when the name matches none or several.
pub fn find(flow: Flow, pattern: &str) -> Result<Endpoint, String> {
    let all = endpoints(flow).map_err(|e| e.to_string())?;
    if let Some(ep) = all.iter().find(|e| e.id == pattern) {
        return Ok(ep.clone());
    }
    let p = pattern.to_lowercase();
    let hits: Vec<&Endpoint> = all.iter().filter(|e| e.name.to_lowercase().contains(&p)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!(
            "no device matches \"{pattern}\"; devices: {}",
            all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")
        )),
        many => Err(format!(
            "\"{pattern}\" matches several devices: {}",
            many.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}
