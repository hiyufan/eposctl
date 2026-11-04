# Architecture and code standards

## Layout

```
src/
  lib.rs, apo.rs, hid.rs, ...   the eposctl library: driver channel, HID protocol,
                                presets, EQ maths, Windows audio. No UI.
  win.rs                        small Win32 helpers (wide strings, registry reads)
  bin/eposctl/                  the command-line tool
    args.rs                     clap syntax only
    commands/                   one module per area: dsp, eqapo, headset, audio, diag
  bin/eposctl-gui/              the tray app
    main.rs                     module list and main()
    app/                        App: wires the components together
      mod.rs                    the struct, construction, per-frame entry points
      background.rs             per-frame work while hidden or shown
      actions.rs                actions shared by pages, tray and hotkeys; file I/O
      pages/                    one file per page or panel (UI only)
    editor.rs  history.rs       models without UI: driver settings with undo and A/B,
    catalog.rs autoswitch.rs    presets and their ids, per-program presets,
    battery.rs meters.rs        battery estimate and alarm, meter smoothing
    config.rs                   persisted settings and the autosave debounce
    worker.rs foreground.rs     background threads: HID, foreground window,
    hotkeys.rs chatmix.rs       global hotkeys, volume mixer,
    devswitch.rs tray.rs        default devices, tray icon
    widgets.rs eq.rs peq.rs     drawing: widgets, the 9-band and parametric editors,
    icons.rs theme.rs           icons, palette and fonts
    instance.rs window.rs       Windows plumbing
    autostart.rs dialog.rs notify.rs
    debug.rs                    debug builds only: screenshot hooks
```

### Rules for where code goes

- **Logic that does not draw goes in a model module**, not in `app/pages`.
  Models take plain values and return plain values (`editor.apply(f) -> bool`,
  `catalog::step(..) -> Option<PresetId>`, `LowAlarm::check(..) -> bool`), so
  they can be unit tested.
- **Pages only draw and route.** A page reads state, draws it, and calls an
  `App` method or a model when the user does something.
- **`App` composes; it does not accumulate.** New features get their own
  component struct (with its own state and methods) and one field on `App`.
  Transient UI state (text being typed, open panels, notices) goes in `View`.
- **Code used by both executables goes in the library.**
- **Threads never touch the UI.** They update shared state or set flags and
  call `request_repaint`; the UI thread acts on them.

## Code standards

Enforced by CI (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`):

| Rule | Limit | Where |
| --- | --- | --- |
| Formatting | rustfmt, 120 columns | `rustfmt.toml` |
| Function length | 80 lines | `clippy.toml` |
| Nesting depth | 5 blocks | `clippy.toml` |
| Cognitive complexity | 20 | `clippy.toml` |
| `let ... else` for early returns, inlined format args, etc. | | `Cargo.toml` `[lints]` |

Run the same checks locally before pushing:

```
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

(On Linux or macOS, `rustup target add x86_64-pc-windows-msvc` and add
`--target x86_64-pc-windows-msvc` to the clippy line to type-check; tests need Windows.)

### Keeping egui code flat

egui closures nest fast. Instead of `panel -> horizontal -> with_layout -> ...`:

- use the layout helpers in `widgets`: `card`, `split_row`, `heading_row`,
  `labeled_row`, `setting_row`, `spaced_divider`, `submit_field`;
- give each panel its own method, and each part of a panel (header actions,
  rows, the add field) its own short method;
- when a closure cannot change `self` because something is borrowed, collect
  what the user asked for in a small struct (`PresetEdits`, `RuleEdits`) and
  apply it after the closure, in its own method;
- return values from closures with `.inner` instead of setting outer
  variables.

### Other conventions

- Use enums and types over flags and tuples: `EqTarget` not `mic: bool`,
  `Notice` not `(String, bool)`, `PresetId` not `"custom3"` strings.
- Early return with `let ... else` instead of a nested `if let`.
- Every `unsafe` block gets a `// SAFETY:` comment saying why it is sound.
- Doc comments say what something is for and anything surprising about it,
  not what the code already says.
