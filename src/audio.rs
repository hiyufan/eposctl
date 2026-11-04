//! Windows audio devices: list them, read the default, and change the default.
//!
//! Changing the default device uses `IPolicyConfig`, an undocumented COM interface
//! that Windows' own sound settings use (as do EarTrumpet and SoundSwitch). It has
//! been stable since Windows 7.

use std::ffi::c_void;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Media::Audio::{
    AudioSessionStateExpired, DEVICE_STATE_ACTIVE, EDataFlow, ERole, IAudioSessionControl2, IAudioSessionManager2,
    IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eCapture, eCommunications, eConsole,
    eMultimedia, eRender,
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

/// One program's audio stream on a device, as listed in the Windows volume mixer.
/// Holds COM objects, so it stays on the thread that listed it.
pub struct Session {
    /// Identifies this stream; stable while the program keeps playing.
    pub id: String,
    /// Lower-case executable name, if the process could be queried.
    pub exe: Option<String>,
    volume: ISimpleAudioVolume,
}

impl Session {
    pub fn volume(&self) -> windows::core::Result<f32> {
        unsafe { self.volume.GetMasterVolume() }
    }

    pub fn set_volume(&self, v: f32) -> windows::core::Result<()> {
        unsafe { self.volume.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null()) }
    }
}

/// Program streams on a device (`None`: the default playback device). The
/// Windows system-sounds stream and expired streams are left out.
pub fn sessions(device: Option<&str>) -> windows::core::Result<Vec<Session>> {
    let e = enumerator()?;
    unsafe {
        let dev = match device {
            Some(id) => e.GetDevice(&HSTRING::from(id))?,
            None => e.GetDefaultAudioEndpoint(eRender, eConsole)?,
        };
        let mgr: IAudioSessionManager2 = dev.Activate(CLSCTX_ALL, None)?;
        let list = mgr.GetSessionEnumerator()?;
        let mut out = Vec::new();
        for i in 0..list.GetCount()? {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            if ctl2.IsSystemSoundsSession() == S_OK || ctl.GetState().is_ok_and(|s| s == AudioSessionStateExpired) {
                continue;
            }
            let (Ok(id), Ok(volume)) = (ctl2.GetSessionInstanceIdentifier(), ctl.cast::<ISimpleAudioVolume>()) else {
                continue;
            };
            let exe = ctl2.GetProcessId().ok().and_then(exe_of_pid);
            out.push(Session { id: take(id), exe, volume });
        }
        Ok(out)
    }
}

/// Lower-case executable file name of a process.
pub fn exe_of_pid(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    if pid == 0 {
        return None;
    }
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(h) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit(['\\', '/']).next().map(str::to_lowercase).filter(|n| !n.is_empty())
}

/// Record from the capture endpoint `id` in shared mode (so the device's APOs
/// process the signal), calling `on_frames(first_channel_samples, sample_rate)`
/// for each packet until `keep_going()` returns false. The mix format must be
/// 32-bit float, which is what Windows uses for shared-mode streams.
pub fn capture(
    id: &str,
    mut keep_going: impl FnMut() -> bool,
    mut on_frames: impl FnMut(&[f32], u32),
) -> windows::core::Result<()> {
    use windows::Win32::Media::Audio::{
        AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, IAudioCaptureClient, IAudioClient,
    };
    let en = enumerator()?;
    unsafe {
        let dev = en.GetDevice(&HSTRING::from(id))?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
        let fmt = client.GetMixFormat()?;
        let (channels, rate, bits) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec, (*fmt).wBitsPerSample);
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 10_000_000, 0, fmt, None);
        CoTaskMemFree(Some(fmt as *const c_void));
        init?;
        if bits != 32 || channels == 0 {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_NOTIMPL,
                "mix format is not 32-bit float",
            ));
        }
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let mut mono = Vec::new();
        while keep_going() {
            std::thread::sleep(std::time::Duration::from_millis(10));
            while cap.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let samples = std::slice::from_raw_parts(data as *const f32, frames as usize * channels);
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                mono.clear();
                mono.extend(samples.chunks(channels).map(|f| if silent { 0.0 } else { f[0] }));
                cap.ReleaseBuffer(frames)?;
                on_frames(&mono, rate);
            }
        }
        client.Stop()?;
    }
    Ok(())
}

/// Keeps a recording stream open on a microphone and discards the samples.
/// The EPOS APO only processes the microphone, and so only updates its level
/// meters, while some program records from it (measured). Stops on drop.
pub struct KeepRecording {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl KeepRecording {
    /// Start on the first active EPOS recording device. `None` if there is none.
    pub fn epos() -> Option<Self> {
        let ep = endpoints(Flow::Input).ok()?.into_iter().find(|e| is_epos(&e.name))?;
        Some(Self::start(ep.id))
    }

    pub fn start(id: String) -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("keep-recording".into())
            .spawn(move || {
                com();
                // Errors (device unplugged, format) just end the stream; the meters stop.
                let _ = capture(&id, || !flag.load(Ordering::Relaxed), |_, _| {});
            })
            .ok();
        Self { stop, thread }
    }
}

impl Drop for KeepRecording {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
