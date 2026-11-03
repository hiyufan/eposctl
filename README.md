# eposctl

Open-source control for EPOS gaming headsets on Windows. It does **not** need
the discontinued EPOS Gaming Suite or its background service.

> Unofficial community project. Not affiliated with or endorsed by EPOS / Demant.
> "EPOS" is a trademark of its respective owner.

EPOS 已停止游戏耳机业务，官方 Gaming Suite 的服务器也已下线。本项目直接与耳机的
音效驱动通信，不依赖官方软件及其后台服务。

## Status

Early. Tested with the **EPOS H3PRO Hybrid** (USB dongle `1395:0303`).

| Feature | Status |
|---|---|
| Speaker 9-band EQ | works |
| Microphone 9-band EQ | works |
| Factory presets (flat / music / esport / movie) | works, exact official curves |
| Virtual 3D surround, reverb, noise gate | works |
| Battery, firmware, serial, product name | works (HID) |
| Sidetone level | works (HID); see note below |
| Live events: ANC on/off, sidetone, headset link | works (`monitor`) |
| Tray app with GUI (`eposctl-gui`) | works |

## Download

Prebuilt binaries are on the [Releases](https://github.com/hiyufan/eposctl/releases)
page: unzip and run `eposctl-gui.exe`.

## Requirements

- Windows 10/11
- The EPOS audio driver (`EPOS APO`) installed. It comes with the Gaming Suite
  installer; once installed, the Gaming Suite application itself is not needed.

No administrator rights are required.

## GUI

`eposctl-gui.exe` is a tray app laid out like the official suite. It has four pages:

- **音效 / Sound**: presets, a draggable 9-band EQ curve, virtual 7.1 surround and reverb
- **麦克风 / Mic**: noise gate, sidetone, microphone EQ
- **设备 / Device**: model, serial, firmware, battery, ANC state
- **设置 / Settings**: language (中文 / English), minimize to tray on close, start with Windows,
  restore the last sound settings on start, low-battery alerts, presets per program,
  global hotkeys, preset import/export

You can save your own presets. Right-click one to overwrite or delete it. The
tray icon's ring shows the battery level, its menu switches presets and turns
surround on or off, and left-clicking it opens the window. Launching the exe
again also brings the running instance to the front.

Ctrl+Z undoes the last sound or mic change (Ctrl+Y redoes). "Compare to before"
switches between the current sound and the sound when you last picked a preset.

**Presets per program**: add a rule for a program (pick it from the recently
used list or type `game.exe`) and the preset changes when that program comes to
the front. Other programs get the "other programs" preset, or nothing changes.
Changes you make by hand stay until you switch to a program with a different
preset.

**Global hotkeys** (off by default): next preset, previous preset and surround
on/off. The defaults are Ctrl+Alt+PageDown, Ctrl+Alt+PageUp and Ctrl+Alt+Home;
click one to record another combination.

**Sound devices follow the headset**: the dongle stays plugged in when the
headset is off, so Windows keeps sending sound to it. With this on, turning the
headset off moves the default playback and recording devices to the ones you
pick (say, the speakers and the built-in mic), and turning it on moves them back.

**Game / voice balance** (Sound page): a software ChatMix. Left makes the game
louder, right makes voice chat louder, using the per-program volumes in the
Windows volume mixer. Volumes you set by hand stay in proportion, and turning it
off puts them back. Voice programs (Discord, KOOK, YY, QQ, WeChat, TeamSpeak, …)
can be edited; everything else counts as game. Two hotkeys can nudge it.

The Device page and the tray tooltip show roughly how long the battery will
last, measured from how fast it drains while the headset is on.

**Preset files**: export saves your custom presets as JSON; import adds the
presets from such a file and skips ones you already have.

**Game presets**: VALORANT, Delta Force, Call of Duty and PUBG presets for
hearing footsteps, tuned for the H3 series. See [docs/game-presets.md](docs/game-presets.md)
for how they were made and the sources. They pair well with per-program presets.

**EQ files**: "Import" next to either EQ loads an Equalizer APO or AutoEQ file
(`ParametricEQ.txt`, `GraphicEQ.txt` or the CSV), so a headphone correction
from [AutoEQ](https://github.com/jaakkopasanen/AutoEq) can be used directly.
The curve is read off at the nine band centres (62.5 Hz to 16 kHz, one octave
apart) and moved up or down to fit the ±6 dB range; the preamp is ignored.
The band frequencies come from the EPOS APO's code, not from a measurement.

EQ curve: drag a point, click in a column to move that band there,
double-click to reset it to 0 dB. You can also click the curve and use the
arrow keys: ←/→ selects a band, ↑/↓ changes it by 0.5 dB, and Shift+↑/↓ by 1 dB.

Settings are stored in `%APPDATA%\eposctl\config.json`. "Start with Windows"
adds `eposctl-gui.exe --minimized` to `HKCU\...\Run`.

## Command line

```text
eposctl list                     # devices exposing an EPOS APO channel
eposctl status [--raw]           # current DSP settings
eposctl preset music             # flat | music | esport | movie
eposctl preset valorant          # game presets: valorant | delta-force | cod | pubg
eposctl eq 6 4 1 0 -1 0 1 4 6    # 9 speaker bands in dB (-6..6), low to high
eposctl mic-eq 0 0 0 0 0 0 0 0 0
eposctl eq-import ParametricEQ.txt [--mic] [--dry-run]   # Equalizer APO / AutoEQ file
eposctl surround on
eposctl reverb 30                # percent
eposctl gate 40                  # percent
eposctl --device 1395:0303 status

eposctl info                     # name, serial, firmware, battery, sidetone (HID)
eposctl sidetone [0-255]         # show or set sidetone level
eposctl monitor [--raw]          # print headset events (read-only)
eposctl report [--query]         # read-only device report for a GitHub issue

eposctl audio                    # Windows playback/recording devices, * = default
eposctl audio default speaker    # make a device the default (part of its name)
eposctl audio default mic --input
eposctl audio sessions           # programs playing on the default device, with volume
```

Sidetone note: on the H3PRO used for testing, sidetone was inaudible at every
level from both the official app and eposctl, although the headset accepts and
reports the value. Reports from other units are welcome.

From Git Bash or other POSIX shells, write the path with forward slashes
(`D:/path/to/eposctl.exe`); backslashes are treated as escapes.

## Other EPOS models

Only the H3PRO Hybrid has been tested. If you have another EPOS headset, run
`eposctl report --query` and open a
[device report](https://github.com/hiyufan/eposctl/issues/new?template=device-report.yml).
The report is read-only and leaves out the serial number. See [ROADMAP.md](ROADMAP.md)
for what is planned.

## Building

```sh
cargo build --release
```

With only MinGW available (no MSVC build tools), use the GNU toolchain:
`rustup override set stable-x86_64-pc-windows-gnu`.

## How it works

The EPOS APO (`EAPO.dll`, loaded by `audiodg.exe`) reads its settings from a
named shared-memory block that the original service used to write. eposctl
writes the same block directly. See [docs/protocol.md](docs/protocol.md) for the
layout and the evidence behind each field.

Prior art: [eposwitch](https://github.com/Ad-Bean/eposwitch) is a control
panel that talks to the official service over its named pipes. Its pipe and HID
notes were a useful reference.

## License

[MIT](LICENSE)
