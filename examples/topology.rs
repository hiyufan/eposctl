//! Probe: dump the Windows audio device topology of EPOS endpoints.
//!
//! Walks every part reachable from each endpoint connector and prints name,
//! type, subtype and — when the part exposes IAudioVolumeLevel — its dB range
//! and current level. Used to locate a hardware sidetone control.

use std::collections::HashSet;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::core::{GUID, Interface, Result};

fn name_of(dev: &IMMDevice) -> Result<String> {
    unsafe {
        let store = dev.OpenPropertyStore(STGM_READ)?;
        let pv = store.GetValue(&PKEY_Device_FriendlyName)?;
        Ok(pv.to_string())
    }
}

fn subtype_name(g: &GUID) -> String {
    // KSNODETYPE_* GUIDs share the {DFF2xxxx-...} family; print raw for now.
    format!("{g:?}")
}

unsafe fn activate<T: Interface>(part: &IPart) -> Result<T> {
    let mut p: *mut std::ffi::c_void = std::ptr::null_mut();
    unsafe {
        part.Activate(CLSCTX_ALL.0, &T::IID, Some(&mut p))?;
        Ok(T::from_raw(p))
    }
}

fn dump_part(part: &IPart, depth: usize, seen: &mut HashSet<(u32, String)>, incoming: bool) -> Result<()> {
    unsafe {
        let local = part.GetLocalId()?;
        let topo_id = part.GetTopologyObject()?.GetDeviceId()?.to_string().unwrap_or_default();
        if !seen.insert((local, topo_id.clone())) {
            return Ok(());
        }
        let name = part.GetName()?.to_string().unwrap_or_default();
        let ptype = part.GetPartType()?;
        let sub = part.GetSubType().map(|g| subtype_name(&g)).unwrap_or_else(|_| "?".into());
        let kind = if ptype == Connector { "Connector" } else { "Subunit" };
        let mut line = format!("{:indent$}[{local:#x}] {kind} {sub} \"{name}\"", "", indent = depth * 2);
        if let Ok(vol) = activate::<IAudioVolumeLevel>(part) {
            let n = vol.GetChannelCount().unwrap_or(0);
            for ch in 0..n {
                let (mut lo, mut hi, mut step) = (0f32, 0f32, 0f32);
                let _ = vol.GetLevelRange(ch, &mut lo, &mut hi, &mut step);
                let cur = vol.GetLevel(ch).unwrap_or(f32::NAN);
                line += &format!("  vol ch{ch}: {cur:.2} dB [{lo:.2}..{hi:.2} step {step:.3}]");
            }
        }
        if let Ok(m) = activate::<IAudioMute>(part) {
            line += &format!("  mute={:?}", m.GetMute().map(|b| b.as_bool()));
        }
        println!("{line}");
        let list = if incoming { part.EnumPartsIncoming() } else { part.EnumPartsOutgoing() };
        if let Ok(list) = list {
            for i in 0..list.GetCount()? {
                dump_part(&list.GetPart(i)?, depth + 1, seen, incoming)?;
            }
        }
        // Follow connectors across filter boundaries.
        if ptype == Connector
            && let Ok(conn) = part.cast::<IConnector>()
                && let Ok(other) = conn.GetConnectedTo()
                    && let Ok(p) = other.cast::<IPart>() {
                        dump_part(&p, depth + 1, seen, incoming)?;
                    }
    }
    Ok(())
}

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        for (flow, label) in [(eRender, "RENDER"), (eCapture, "CAPTURE")] {
            let col = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
            for i in 0..col.GetCount()? {
                let dev = col.Item(i)?;
                let name = name_of(&dev).unwrap_or_default();
                if !name.to_lowercase().contains("epos") {
                    continue;
                }
                println!("=== {label}: {name}");
                let topo: IDeviceTopology = dev.Activate(CLSCTX_ALL, None)?;
                for c in 0..topo.GetConnectorCount()? {
                    let conn = topo.GetConnector(c)?;
                    let mut seen = HashSet::new();
                    println!("-- incoming from endpoint connector {c}");
                    if let Ok(other) = conn.GetConnectedTo() {
                        dump_part(&other.cast::<IPart>()?, 1, &mut seen, true)?;
                    }
                    println!("-- outgoing from endpoint connector {c}");
                    let mut seen = HashSet::new();
                    if let Ok(other) = conn.GetConnectedTo() {
                        dump_part(&other.cast::<IPart>()?, 1, &mut seen, false)?;
                    }
                }
            }
        }
    }
    Ok(())
}
