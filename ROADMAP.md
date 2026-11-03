# Roadmap

Plans, not promises. Items that need reverse engineering are marked **RE**.

## 0.1.1: release basics

- [x] CI build, tagged releases with prebuilt binaries
- [x] `eposctl report` and a device-report issue template, to collect data for other models
- [x] MOVIE preset curve

## 0.2: everyday features

- [ ] Live microphone level meter on the Mic page, with the noise-gate threshold drawn on it
      (**RE**: map the 15 APO meters; check whether the mic monitor flags must be set)
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

- [ ] `eposctl raw`: send read-only HID requests to decode the unknown getters
      (`0A/12`, `0A/17`, `01/14`, `06/14`). Setters (`0A/11`, `0A/16`, `06/32`, `01/1F`)
      are tried one at a time with read-back, because they may persist on the headset.
      Likely candidates: auto power-off timer, prompt volume, mute tone, ANC control.
- [x] Centre frequencies of the 9 EQ bands (62.5 Hz to 16 kHz, octaves; from `EAPO.dll`),
      and AutoEQ / Equalizer APO import
- [ ] Filter shape of the bands (`EAPO.dll` passes 1.5 and 0.95 to its EQ; probably Q and
      a level factor), to fit imported curves exactly instead of sampling them
- [ ] Remaining APO fields (`hrtf_preset`, `0xB4`, the mic processing flags)

## Later

- More EPOS models, driven by device reports

## Not planned

- Own audio processing (AI noise suppression, parametric EQ, a custom APO or virtual
  device). Pair eposctl with Equalizer APO or NVIDIA Broadcast instead.
