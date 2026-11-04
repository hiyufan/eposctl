# EPOS APO settings channel

Reverse-engineering notes for the channel between the EPOS Gaming Suite service
(`EPOSGamingSuiteService`, 1.12.2.1185) and the EPOS audio processing object
(`EAPO.dll`, shipped with the `eposaudio.inf` driver package).

Everything here was established on Windows 11 with an EPOS H3PRO Hybrid on its
USB dongle (VID `0x1395`, PID `0x0303`, dongle firmware 3.12.1).

Each claim is marked:

- **observed**: seen directly (object listing, memory diff while using the official UI)
- **inferred**: consistent with observations but not isolated
- **unknown**: open question

## Transport

The service and the APO share a block of memory through named kernel objects.
They do **not** use the endpoint `FxProperties` property store. The
`{A5A78EA4-…}`-style values under the EPOS endpoint's `FxProperties` key are also
present on Realtek endpoints, so they are generic Windows/vendor enhancement
settings, not EPOS parameters (observed).

| Object | Name | Notes |
|---|---|---|
| Section | `Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_memory` | 4096-byte page, ~0x47C bytes used (observed) |
| Mutant  | `Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_mutex`  | guards the block (inferred from RTTI `epos::ipc::shared_mutex`) |
| Event   | `Global\CF4B411F-BE2B-4D84-8106-EC27CA0F8F05_<VID>_<PID>_event`  | change notification (inferred from RTTI `epos::ipc::shared_event`) |

`<VID>`/`<PID>` are 4-digit hex. Both binaries contain RTTI for
`epos::ipc::shared_memory_ipc<epos::ipc::apo_settings::Data>`; the APO also has
`shared_memory_ipc_listener<…>` (observed in strings).

All three objects can be opened for read **and write** by a normal,
non-elevated user (observed).

Both processes run as `NT AUTHORITY\LocalService` (observed: service account of
`EPOSGamingSuiteService` and `Audiosrv`).

## Settings block layout (`apo_settings::Data`)

Offsets are from the start of the section. Floats are little-endian IEEE-754.
"named" = the field's name and width come from the settings (de)serialiser in
`EAPO.dll` (RVA `0x2E6650` reads each field from the registry by name into
`[struct + offset]`, RVA `0x2E6C20` writes them back); its effect on the H3PRO
has not been tested unless marked "measured". `eposctl status --raw` prints them.

"measured" = recorded from the headset mic through Windows while switching the
field (`examples/miclab.rs`, 2026-11-04, H3PRO Hybrid). The official UI set
`0x42`, `0x44` and `0x46` only while its microphone page was open and cleared
them on leaving, so its mic EQ and gate only acted on that page. eposctl
switches `0x42` and `0x44` with the EQ gains and the gate amount instead.

Gate (with `0x44` on), measured with speech and with music played into the mic:

- It follows a slow level average, not single 20 ms blocks: music averaging
  -38 dBFS kept it open through -61 dBFS passages at 40 %, while quiet speech
  averaging -60 dBFS was shut at 35 %.
- Closed, it attenuates by roughly 50 dB rather than muting.
- It acts after the mic EQ: lowering the signal by about 10 dB with the EQ made
  it close at lower amounts.
- Rough threshold: -72 + 45 x amount dBFS, within about ±5 dB at 20-60 % (20 %
  about -63, 35 % about -56, 60 % about -45). Music is too uneven to pin it
  down further.

Speech compressor presets 0-5 made no measurable difference on music around
-36 dBFS; on quiet speech around -55 dBFS, preset 2 lowered it by about 20 dB.
It behaves more like a downward expander for quiet input than a leveller.

| Offset | Type | Meaning | Status |
|---|---|---|---|
| `0x00` | u32 | `2` in every capture; probably a version | observed value, meaning unknown |
| `0x04` | u32 | `4` in every capture | unknown |
| `0x40` | u8 | virtual 3D surround enabled; `EAPO.dll` calls it `direct_sound_enabled` | observed |
| `0x41` | u8 | `speaker_eq_enabled`; `1` in every capture | named |
| `0x42` | u8 | `mic_eq_enabled`: the mic EQ gains (`0x70`) only act while it is 1 | measured |
| `0x44` | u8 | `noise_gate_enabled`: the gate amount (`0x98`) only acts while it is 1 | measured |
| `0x46` | u8 | `mic_meters_enabled`: the APO updates the meters at `0x440` only while it is 1 **and** a program records from the mic | measured |
| `0x43` | u8 | `mic_nr_enabled`; no measurable effect on the H3PRO | measured |
| `0x47` | u8 | `mic_hpf_enabled`; no measurable effect on the H3PRO | measured |
| `0x45` | u8 | `reverb_enabled`; always flipped together with `0x40` by the official UI | observed + named |
| `0x4C` | f32[9] | speaker EQ gains in dB, -6..+6, lowest band first | observed |
| `0x70` | f32[9] | microphone EQ gains in dB | observed (bands 1-8) |
| `0x94` | f32 | reverb amount, 0..1 (official UI writes slider/255) | observed |
| `0x98` | f32 | noise gate amount, 0..1 (official UI writes slider/255) | observed |
| `0x9C` | u32 | `mic_eq_preset` | named |
| `0xA0` | u32 | `mic_nr_preset` | named |
| `0xA8` | u32 | `hrtf_preset` | named |
| `0xB1` | u8 | `mic_speech_compressor_enabled`; with preset 2 it made speech about 20 dB quieter and gated pauses | measured |
| `0xB4` | u32 | `mic_speech_compressor_preset`; `2` in every capture | named |
| `0x440` | f32[15] | level meters in dB written by the APO; see below | observed |

### Level meters (`0x440`)

With the monitor flags at `0x42/0x44/0x46` cleared, all 15 values stay frozen at
their last value (observed). With them set:

- meters 0-8 update continuously between -60 and 0 dB (floor -60). Talking into
  the microphone raised meters 1-5 by 6-16 dB on average over silence (observed,
  one guided capture). They look like a per-band spectrum, lowest band first,
  probably matching the 9 microphone EQ bands (inferred). Meter 8 stayed at -60.
  Whether playback also feeds them has **not** been checked yet.
- meters 9-14 stayed at -120 in every capture (unknown).

`examples/meters.rs` runs the guided capture (silence, speech, playback).

### EQ bands

`EAPO.dll` (2023-05-02 build) builds a table of nine floats at start-up,
`62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000` (constants at RVA
`0x4E7360` and `0x4D6B00`, static initialiser at RVA `0x258190`), and passes it
to its EQ together with the sample rate, a sample-rate dependent coefficient
table (one for 48 kHz, one for other rates) and the constants `1.5` and `0.95`
(EQ setup at RVA `0x2B11D3`). It is the only 9-entry frequency table in the
DLL, so these are taken as the band centres of both EQs (inferred). `1.5` is
probably the bands' Q and `0.95` a level factor; not confirmed.

Setting names found in both binaries (offsets in the table above):
`direct_sound_enabled`, `speaker_eq_enabled`, `speaker_eq_levels`,
`reverb_enabled`, `reverb_level`, `noise_gate_enabled`, `noise_gate_level`,
`hrtf_preset`, `mic_eq_enabled`, `mic_eq_levels`, `mic_eq_preset`,
`mic_hpf_enabled`, `mic_meters_enabled`, `mic_nr_enabled`, `mic_nr_preset`,
`mic_speech_compressor_enabled`, `mic_speech_compressor_preset`.

`Software\EPOS\SCAPO` appears in both binaries and probably persists these
settings in the LocalService hive (`HKU\S-1-5-19`). This has not been checked
because the hive is not readable without elevation.

## Write protocol used by eposctl

1. Open the section, mutex and event by name.
2. Wait on the mutex (an abandoned mutex counts as acquired).
3. Write only the 32-bit words that changed, never the meter area at `0x440`.
4. Release the mutex, then `SetEvent`.

This works while the official service is still running: a speaker EQ written
this way was audible at once (bass boost with treble cut sounded muffled), and
the service had not overwritten it 3 seconds later (observed).

## Factory presets (as written by the official UI)

The preset XML files hold 5 sliders (-127..127); the APO has 9 bands. These are
the 9-band values the official UI actually wrote:

| Preset | Speaker EQ (dB) | Reverb | Gate | Surround |
|---|---|---|---|---|
| FLAT   | 0 0 0 0 0 0 0 0 0 | 0 | 100/255 | off |
| MUSIC  | 6 3.97 0.85 -0.43 -1.04 -0.47 0.66 4.02 6 | 0 | 40/255 | off |
| ESPORT | -6 -3.97 -0.85 0.85 2.03 4.2 6 6 6 | 77/255 | 90/255 | on |
| MOVIE  | 6 4.16 1.28 0.28 0 0 0.66 4.02 6 | 0 | 170/255 | on |

The official preset files (`XMLFolder\*PresetEQSettings.xml` in the install
directory, copies under `%ProgramData%\EPOS\Gaming Suite\CUSTOMPRESET`) agree
with these captures for reverb (`ReverbKnob`), surround (`Audio3D`) and noise
gate (`NoiseGate`). Their five EQ sliders (-127..127) map to the nine bands as
follows (observed):

| Preset | Sliders 1-5 | `micEQ` |
|---|---|---|
| FLAT   | 0 0 0 0 0 | 0 |
| MUSIC  | 127 0 -22 0 127 | 1 |
| ESPORT | -127 0 43 127 127 | 0 |
| MOVIE  | 127 11 0 0 127 | 1 |

The files also have `ECOMode` (0 in all four) and a `VoiceEnhancer`/`micEQ`
flag whose effect is not known.

Selecting ESPORT also wrote a mic EQ curve
(`6 6 4.02 -0.52 -3.02 -2.31 1.04 -3.02 0`). MUSIC did not, so how presets
relate to the mic EQ is still unknown.

## HID protocol ("Capricorn" datagrams)

The service's H3PRO adapter is `CCapricornDatagram` (strings: `sendCommand`,
`getHIDBuffers`, `verifyOpcode`, `GetSideToneGain`). `DeviceInfoConfig.txt`
maps the H3PRO family to `"FWAdapter":"Capricon"`.

Transport: dongle interface 3, collection 1, usage page `0xFFFF` / usage
`0x0001`. Report ID `0x01`, 106-byte payload in both directions, no feature
reports (observed via `HidP_GetCaps`). Each open handle receives its own copy
of input reports, so responses to the official service's requests can be read
passively.

Frame layout (all u16 little-endian, observed on 23 frames):

| Offset | Field | Values seen |
|---|---|---|
| 0 | report id | `0x01` |
| 1 | magic | `0xFEEB` |
| 3 | version? | `0x0001` |
| 5 | kind | `0x0004` response, `0x010F` notification, `0x0110` link event, `0x2004` unsupported, `0x3005` not delivered, `0x01FF` ? |
| 7 | node | `0` before the headset links, `4` after (inferred: "slave" address, 4 = headset) |
| 9 | body length | bytes |
| 11 | check | CRC-8 of the body: poly `0x07`, init `0xFF`, no reflection, no final xor (the only fit among all CRC-8 parameter sets, sums and xors tried) |
| 13 | body | u16 group, u16 command, then values |

Decoded messages (observed: each one lined up with a user action or with the
service's query burst right after the headset reconnected):

| Kind | Group | Cmd | Values | Meaning |
|---|---|---|---|---|
| `0x0110` | `01` | `F0` | `1, connected` | headset link up/down |
| `0x0004` | `09` | `01` | UTF-16 | product name ("EPOS H3PRO") |
| `0x0004` | `01` | `0B` | UTF-16 | serial number |
| `0x0004` | `01` | `03` | `major, minor, patch` | headset firmware (1.3.5) |
| `0x0004` | `12` | `03` | `percent` | battery level (100) |
| `0x0004` | `0A` | `0B` | `status` | sidetone set acknowledged (0) |
| `0x010F` | `0A` | `F0` | `2, raw` | sidetone changed. The official slider at 0 gave `0xFF`, at max gave `0x00`, mid gave `0x8B` |
| `0x010F` | `17` | `07` | `0/1` | ANC off/on (headset button) |
| `0x0004` | `0A` | `0C` | `6` | sidetone level, queried on connect |
| `0x0004` | `0A` | `12` | `1` | noise reduction level, queried on connect |
| `0x0004` | `01` | `20` | `1` | read side of setter `01/1F`, meaning unknown; queried on connect |
| `0x0004` | `01` | `14` | `0x0101` | PID number (`parsePIDNumber`), queried on connect |
| `0x0004` | `06` | `14` | 4 words | Bluetooth address (`parseBDAddr`), queried on connect. Identifies the unit; eposctl redacts it |
| `0x01FF` | `08` | `F0` | `3, 0…` | unknown, seen once when other headset buttons were pressed |
| `0x3005` | (request's) | (request's) | none | from node 1 (the dongle) right after each request while the headset was off; never seen while it was linked, so "not delivered" |
| `0x2004` | (request's) | (request's) | `0, 0` | from the headset for a request it does not support (`0A/17` on the H3PRO) |

The service also logs internal opcode numbers when a write fails (0, 1, 3, 4,
14, 16, 19, 20, 27); these are enum values inside the service, not wire values.

### Requests

Recovered with Ghidra 12.1.4 from `EPOSGamingSuiteService` 1.12.2
(`CCapricornDatagram::EncodeDatagram` @ `0x4ce570`, `getHIDBuffers` @ `0x4c7b40`):

- A request uses **the same frame as a response**: report id 1, `0x0001FEEB`,
  kind `4`, node (caller-supplied; the headset is `4`), body length, CRC byte at
  offset 11 (offset 12 stays 0), body at offset 13.
- `getHIDBuffers` maps the service's internal opcode to a constant 4-byte body
  `(u16 group, u16 command)` from a table at VA `0x826F1C`–`0x826F93`. For
  opcodes `0x11`, `0x12`, `0x16`, `0x18`, `0x1A` it appends one value (parsed
  from a decimal string, written as u16), making the body 6 bytes.

| Opcode | Group, Cmd | Use | Status |
|---|---|---|---|
| 0 | `01, 03` | firmware version | verified by eposctl |
| 1 | `01, 0B` | serial number | verified by eposctl |
| 2 | `09, 01` | product name | verified by eposctl |
| 3 | `06, 14` | Bluetooth address (`parseBDAddr`) | answered on connect |
| 4 | `01, 09` | sub-devices connected (`parsesubDevicesConnected`) | |
| 5 | `01, 0A` | no parser ("Opcode not supported"); may be an action | not sent |
| 6 | `01, 0D` | firmware update type (`parseDfuType`) | not sent |
| 7 | `01, 06` | **enter firmware update mode** (`parseEnterDfu`) | never sent; `eposctl raw` refuses it |
| 9 | `06, 09` + 10 zero bytes | no parser; the 10-byte argument suggests a pairing/connect action | not sent |
| 10 | `06, 13` | no parser; may be an action | not sent |
| 12 | `06, 27` | Bluetooth name (`parseDeviceBtName`) | |
| 14 | `12, 03` | battery percent (`GetBatPercentage` reads offsets 0x11–0x12) | verified by eposctl |
| 16 | `0A, 0C` | get sidetone (`GetSideToneGain` reads offset 0x11) | verified by eposctl |
| 17 | `0A, 0B` + value | set sidetone | verified by eposctl (ack status 0, read-back matches) |
| 18 | `0A, 11` + value | set microphone noise reduction, 0 off / 1 medium / 2 strong | verified by eposctl (ack status 0, read-back matches; 2 then back to 1) |
| 19 | `0A, 12` | get microphone noise reduction (`GetNoiseReductionLevel` reads offset 0x11); `1` on the H3PRO by default | verified by eposctl |
| 20 | `01, 14` | PID number (`parsePIDNumber`) | answered on connect (`0x0101`) |
| 21 | `08, F0` | no parser | not sent |
| 22 | `0A, 16` + value | set latency level (inferred: pairs with 23) | not tried |
| 23 | `0A, 17` | get latency level (`GetLatencyLevel` reads offset 0x13, signed) | unsupported on H3PRO (kind `0x2004` reply) |
| 24 | `06, 32` + value | remote device info (`GetRemoteDeviceInfo`); the value is probably an index, so a query rather than a setter (inferred) | |
| 25 | `06, 11` | number of link keys, i.e. paired devices (`GetNbrLinkKeys`) | |
| 26 | `01, 1F` + value | setter; its reply is parsed like 27 (byte at offset 0x11) | not tried |
| 27 | `01, 20` | getter for 26 (byte at offset 0x11), meaning unknown | answered on connect (`1`) |
| 28 | `06, 1C` | reply is not parsed for a value (case 28 yields the string "0"); may be an action | not sent |

**Noise reduction in the official UI** (`EPOSGamingSuite.exe`,
`MicrophoneViewModel.<NoiseCancellation_PreviewMouseLeftButtonUp>b__0`): a slider
with `Minimum 0`, `Maximum 2`, `TickFrequency 1` (BAML `ucnoisecancellation`).
If the headset reports noise reduction level `-1`, the UI writes the value to the
APO's `mic_nr_preset` (`0xA0`) instead, a software fallback; otherwise it sends
request type 5 with the value to the service, which becomes opcode 18. On the
H3PRO the level is `1`, so noise reduction runs in the headset, which is why the
APO's `mic_nr_enabled` had no measurable effect.

**Auto sleep in the official UI** (`UCPlaybackDevice.<Lblautosleep_PreviewMouseLeftButtonUp>b__35_0`):
an on/off toggle that sends request type 7 with `1` (on) or `0` (off); the UI
hides it unless the service reports a status of 0 or 1. On the wire that is
`DeviceAutoSleepLevel` below. eposctl does not send it yet: there is no getter, so
the current value could not be put back.

The meaning of opcodes 18/19/22/23 comes from the response parser: the
service's response dispatcher switches on the internal opcode (jump table at
VA `0x4C8DF0`), and case 19 calls `GetNoiseReductionLevel`, case 23
`GetLatencyLevel`, case 16 `GetSideToneGain`, case 14 `GetBatPercentage`. The
value ranges are not known; the strings of other device families
(`NoiseReductionOff/Average/Max`, `BtMinLinkLatency/BtMaxLinkLatency`) suggest
small enums, so `1` from opcode 19 may mean "average". This is a guess.

The other parser names (`parseBDAddr`, `parsesubDevicesConnected`,
`parseDfuType`, `parseEnterDfu`, `parseDeviceBtName`, `parsePIDNumber`,
`GetRemoteDeviceInfo`, `GetNbrLinkKeys`) come from the same dispatcher
(`parseDatagram` @ `0x4c8330`): cases 0–4, 6, 7, 12, 14, 16, 17–19, 20, 22–23,
24, 25, 26–27 call a parser; cases 5, 8–11, 13, 15 and 21 fall to "Opcode not
supported". `eposctl raw --probe` sends only the getter-style requests;
`eposctl raw` refuses opcode 7 and needs `--write` for values and for the
requests without a parser.

**Auto sleep** does not use the opcode table. `CCapricornDatagram::DeviceAutoSleepLevel`
(`0x4CF820`, slot 3 of the vtable at `0x828DB0`) builds its own 8-byte body and
sends it through `EncodeDatagram`:

```
08 00  EF 00  01 00  <level> 00      (u16 group 0x08, u16 cmd 0xEF, u16 1, u16 level)
```

It returns a u16 status from the response. What `level` means (minutes, an
index, or on/off) is unknown, and it is not known whether the H3PRO supports
it. Not tried by eposctl.

Sidetone values: the official UI sent `0xFF` at its slider minimum, `0x00` at
its maximum and `0x8B` in the middle. eposctl's writes of 0, 255 and 139 were
acknowledged and read back correctly. On the test unit **no sidetone was
audible at any value, from the official UI or from eposctl**, even with the
microphone open. eposctl's request is byte-identical to the official one (the
capture shows only the `0A/0B` acknowledgement per slider move), so the cause is
on the device side and is not known.

## Not in this channel

- **Sidetone**: moving the slider changed nothing in the block (observed). It goes to the dongle over HID (see above).
  The Windows device topology of the dongle endpoints has no sidetone node
  either (render: volume, DAC; capture: mute, volume, ADC).
- **Battery, ANC state, firmware version**: HID (see above).
- Mic noise reduction / HPF toggles in the official UI produced no change (observed). The H3PRO may not support them.

## Open questions

- Does the APO create the section itself when the EPOS service is not running?
  (`EAPO.dll` contains "Could not create shared memory", which suggests create-or-open.)
- Meaning of `0x00`, `0x04`, `0x48`, `0xA4`, `0xAC`, `0xB0`, and the bytes between `0xB8` and `0x440`.
- The gate's percent-to-dB threshold mapping (needs a steady test signal).
- What the speech compressor presets other than 2 do; `hrtf_preset` values.
- Whether the event must be signalled for the APO to pick up changes, or if it polls.
