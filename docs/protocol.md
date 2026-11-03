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

| Offset | Type | Meaning | Status |
|---|---|---|---|
| `0x00` | u32 | `2` in every capture; probably a version | observed value, meaning unknown |
| `0x04` | u32 | `4` in every capture | unknown |
| `0x40` | u8 | virtual 3D surround enabled | observed |
| `0x41` | u8 | `1` in every capture; probably speaker EQ enabled | inferred |
| `0x42`, `0x44`, `0x46` | u8 | set while the official UI shows the microphone page; probably mic meters / voice activity / voice peak monitors | inferred |
| `0x45` | u8 | always flipped together with `0x40` by the official UI; probably reverb enabled | inferred |
| `0x4C` | f32[9] | speaker EQ gains in dB, -6..+6, lowest band first | observed |
| `0x70` | f32[9] | microphone EQ gains in dB | observed (bands 1-8) |
| `0x94` | f32 | reverb amount, 0..1 (official UI writes slider/255) | observed |
| `0x98` | f32 | noise gate amount, 0..1 (official UI writes slider/255) | observed |
| `0xB4` | u32 | `2` in every capture; candidate for `hrtf_preset` or `mic_nr_preset` | unknown |
| `0x440` | f32[15] | level meters in dB written by the APO; -120 = silence | observed |

Setting names found in both binaries (the order in the struct is not known):
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
| MOVIE  | not captured yet | | | |

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
| 5 | kind | `0x0004` response, `0x010F` notification, `0x0110` link event, `0x01FF` ? |
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
| `0x0004` | `0A` | `0C` | `6` | unknown, queried on connect |
| `0x0004` | `0A` | `12` | `1` | unknown, queried on connect |
| `0x0004` | `01` | `20` | `1` | unknown, queried on connect |
| `0x0004` | `01` | `14` | `0x0101` | unknown, queried on connect |
| `0x0004` | `06` | `14` | `0x6c, 0x7532, 0x94, 0x16` | unknown, queried on connect |
| `0x01FF` | `08` | `F0` | `3, 0…` | unknown, seen once when other headset buttons were pressed |

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
| 3 | `06, 14` | (BD address?) | answered on connect |
| 4 | `01, 09` | | |
| 5 | `01, 0A` | | |
| 6 | `01, 0D` | | |
| 7 | `01, 06` | | |
| 9 | `06, 09` + 10 zero bytes | | |
| 10 | `06, 13` | | |
| 12 | `06, 27` | | |
| 14 | `12, 03` | battery percent (`GetBatPercentage` reads offsets 0x11–0x12) | verified by eposctl |
| 16 | `0A, 0C` | get sidetone (`GetSideToneGain` reads offset 0x11) | verified by eposctl |
| 17 | `0A, 0B` + value | set sidetone | verified by eposctl (ack status 0, read-back matches) |
| 18 | `0A, 11` + value | setter, unknown | |
| 19 | `0A, 12` | getter, unknown; returns `1` on H3PRO | read by eposctl |
| 20 | `01, 14` | | answered on connect (`0x0101`) |
| 21 | `08, F0` | | |
| 22 | `0A, 16` + value | setter, unknown | |
| 23 | `0A, 17` | getter, unknown | no response on H3PRO |
| 24 | `06, 32` + value | setter, unknown | |
| 25 | `06, 11` | | |
| 26 | `01, 1F` + value | setter, unknown | |
| 27 | `01, 20` | | answered on connect (`1`) |
| 28 | `06, 1C` | | |

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
- Meaning of `0x00`, `0x04`, `0x48`, `0xB4`, and the bytes between `0x9C` and `0x440`.
- MOVIE preset curve.
- Whether the event must be signalled for the APO to pick up changes, or if it polls.
