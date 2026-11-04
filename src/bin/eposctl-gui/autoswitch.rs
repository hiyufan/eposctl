//! Per-program presets: which preset the program in front asks for.

use std::time::Duration;

use eframe::egui;

use crate::config::AutoPresets;
use crate::foreground;

/// What to do after looking at the foreground program.
pub enum Next {
    Nothing,
    /// A program just came to the front; look again after this long.
    WaitFor(Duration),
    /// Apply this preset (a rule reference, see `catalog::PresetId::from_rule`).
    Apply(String),
}

pub struct AutoSwitch {
    fg: foreground::Shared,
    /// Foreground program the rules were last evaluated for.
    app: Option<String>,
    /// Preset those rules picked (`None`: leave the sound alone).
    target: Option<String>,
}

impl AutoSwitch {
    pub fn spawn(ctx: egui::Context) -> Self {
        Self { fg: foreground::spawn(ctx), app: None, target: None }
    }

    /// Act on the program in front once it has stayed there a moment. Only a
    /// change of *target preset* applies anything, so hand-made changes survive
    /// switching between programs that share a preset.
    pub fn poll(&mut self, cfg: &AutoPresets) -> Next {
        let Some((exe, since)) = self.fg.lock().unwrap().current.clone() else { return Next::Nothing };
        if self.app.as_deref() == Some(exe.as_str()) {
            return Next::Nothing;
        }
        let wait = foreground::SETTLE.saturating_sub(since.elapsed());
        if !wait.is_zero() {
            return Next::WaitFor(wait);
        }
        let target = cfg.rules.iter().find(|r| r.exe == exe).map(|r| r.preset.clone()).or_else(|| cfg.default.clone());
        self.app = Some(exe);
        if target == self.target {
            return Next::Nothing;
        }
        self.target = target.clone();
        target.map_or(Next::Nothing, Next::Apply)
    }

    /// Switched on: take effect from the next switch rather than changing the
    /// sound right now.
    pub fn restart(&mut self) {
        self.app = self.fg.lock().unwrap().current.as_ref().map(|(e, _)| e.clone());
        self.target = None;
    }

    /// The rules changed: re-evaluate on the next switch.
    pub fn rules_changed(&mut self) {
        self.target = None;
    }

    /// Recently focused programs that have no rule yet, most recent first.
    pub fn recent_without_rule(&self, cfg: &AutoPresets) -> Vec<String> {
        let fg = self.fg.lock().unwrap();
        fg.recent.iter().filter(|e| !cfg.rules.iter().any(|r| r.exe == **e)).cloned().collect()
    }
}
