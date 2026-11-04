# Roadmap

Plans, not promises. Items that need reverse engineering are marked **RE**.

## 0.1.1: release basics

- [x] CI build, tagged releases with prebuilt binaries
- [x] `eposctl report` and a device-report issue template, to collect data for other models
- [x] MOVIE preset curve

## 0.2: everyday features

- [x] Live microphone level meter and spectrum on the Mic page (APO meters at `0x440`,
      with the mic monitor flags switched on while the page is open)
- [ ] Noise-gate threshold drawn on the meter (needs the gate's percent-to-dB mapping)
- [x] Low-battery notification at 20 / 10 / 5 %, once per level, reset when charged
- [x] Surround toggle in the tray menu
- [x] EQ undo and A/B compare

## 0.3: automation

- [x] Per-application presets: switch preset when a given program comes to the foreground,
      with a default preset. Manual changes are kept until the foreground program changes.
- [x] Global hotkeys (next/previous preset, surround on/off)
- [x] Export and import presets as JSON

## 0.4: things the official suite never had

- [x] Switch the Windows default playback/recording devices when the headset is turned off
      and back on (the dongle stays plugged in, so Windows does not notice on its own)
- [x] Battery runtime estimate from the measured drain rate
- [x] Game/voice balance over the per-program volumes in the Windows mixer (software ChatMix),
      with hotkeys

## 0.5: games

- [x] Footstep presets for VALORANT, Delta Force, Call of Duty and PUBG
      (from community sources, tuned for the H3 series; not yet tried in game)
- [x] Import of Equalizer APO / AutoEQ files

## Research track (runs alongside the above)

- [x] Microphone noise reduction in the headset (`0A/11`, 0-2; values from the official UI),
      in the CLI (`eposctl noise-reduction`) and on the Mic page
- [ ] Auto sleep on/off (`08/EF`; values known, but no getter to read the current state)
- [x] `eposctl raw` (and `raw --probe`): send read-only HID requests to decode the unknown getters
      (`0A/12`, `0A/17`, `01/14`, `06/14`). Setters (`0A/11`, `0A/16`, `06/32`, `01/1F`)
      are tried one at a time with read-back, because they may persist on the headset.
      Likely candidates: auto power-off timer, prompt volume, mute tone, ANC control.
- [x] Centre frequencies of the 9 EQ bands (62.5 Hz to 16 kHz, octaves; from `EAPO.dll`),
      and AutoEQ / Equalizer APO import
- [x] How the bands interact (the APO's 9x9 interaction matrix, from `EAPO.dll`), so imported
      curves are least-squares fitted instead of sampled at the centres
- [x] Parametric EQs with more bands than nine: exact via Equalizer APO (`eposctl eqapo`)
- [ ] Parametric EQ editor in the GUI (any number of bands; 9-band fit or Equalizer APO)
- [x] Offsets of the remaining APO fields (`hrtf_preset`, noise reduction, HPF, speech
      compressor; from the settings serialiser in `EAPO.dll`)
- [x] Measure them on the H3PRO: the gate and the mic EQ need their enable flags
      (fixed); noise reduction and HPF do nothing measurable; the compressor (preset 2)
      makes speech much quieter
- [ ] `hrtf_preset` values and the other compressor presets

## Living next to the official software

- [x] Switch the Gaming Suite's service and app off (and back on) from Settings or
      `eposctl suite`, keeping the driver: the APO creates its shared memory itself

## Distribution

- [x] Installer (Inno Setup, per user, no admin): Start menu, optional start with Windows,
      uninstaller; offers Equalizer APO (pinned version and SHA-256) when the EPOS driver is missing
- [x] Continuous delivery: bumping the version in Cargo.toml on main releases it (installer,
      exes, zip, checksums); also a manual "Run workflow" button and v* tags
- [ ] Code signing (removes the SmartScreen warning; needs a certificate)

## Later

- More EPOS models, driven by device reports

## Not planned

- Own audio processing (AI noise suppression, parametric EQ, a custom APO or virtual
  device). Pair eposctl with Equalizer APO or NVIDIA Broadcast instead.
