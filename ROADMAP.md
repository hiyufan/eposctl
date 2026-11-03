# Roadmap

Plans, not promises. Items that need reverse engineering are marked **RE**.

## 0.1.1: release basics

- [x] CI build, tagged releases with prebuilt binaries
- [x] `eposctl report` and a device-report issue template, to collect data for other models
- [ ] MOVIE preset curve (**RE**: capture from the official app)

## 0.2: everyday features

- [ ] Live microphone level meter on the Mic page, with the noise-gate threshold drawn on it
      (**RE**: map the 15 APO meters; check whether the mic monitor flags must be set)
- [ ] Low-battery notification at 20 / 10 / 5 %, once per level, reset when charged
- [ ] Surround toggle in the tray menu
- [ ] EQ undo and A/B compare

## 0.3: automation

- [ ] Per-application presets: switch preset when a given program comes to the foreground,
      with a default preset. Manual changes are kept until the foreground program changes.
- [ ] Global hotkeys (next/previous preset, surround on/off)
- [ ] Export and import presets as JSON

## Research track (runs alongside the above)

- [ ] `eposctl raw`: send read-only HID requests to decode the unknown getters
      (`0A/12`, `0A/17`, `01/14`, `06/14`). Setters (`0A/11`, `0A/16`, `06/32`, `01/1F`)
      are tried one at a time with read-back, because they may persist on the headset.
      Likely candidates: auto power-off timer, prompt volume, mute tone, ANC control.
- [ ] Centre frequencies of the 9 EQ bands; required before AutoEQ / Equalizer APO import
- [ ] Remaining APO fields (`hrtf_preset`, `0xB4`, the mic processing flags)

## Later

- Voice/game volume balance using Windows per-application volume (not a real ChatMix)
- More EPOS models, driven by device reports
- Actions on headset connect/disconnect

## Not planned

- Own audio processing (AI noise suppression, parametric EQ, a custom APO or virtual
  device). Pair eposctl with Equalizer APO or NVIDIA Broadcast instead.
