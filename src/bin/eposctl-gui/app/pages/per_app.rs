//! Per-program presets panel: a preset for each program, and one for the rest.

use eframe::egui::Ui;

use super::super::App;
use super::exe_input;
use crate::catalog;
use crate::config::AppRule;
use crate::widgets;

/// What the rule list asked for this frame.
#[derive(Default)]
struct RuleEdits {
    set: Option<(usize, String)>,
    remove: Option<usize>,
    add: Option<String>,
}

/// A preset drop-down. `none` adds a first entry meaning "no preset". True when
/// the choice changed.
pub(super) fn preset_combo(
    ui: &mut Ui,
    id: impl eframe::egui::AsIdSalt,
    value: &mut Option<String>,
    choices: &[(String, String)],
    none: Option<&str>,
    missing: &str,
) -> bool {
    let before = value.clone();
    let text = match value.as_deref() {
        None => none.unwrap_or_default().to_owned(),
        Some(v) => choices.iter().find(|(k, _)| k == v).map_or_else(|| missing.to_owned(), |(_, l)| l.clone()),
    };
    eframe::egui::ComboBox::from_id_salt(id).selected_text(text).width(150.0).show_ui(ui, |ui| {
        if let Some(n) = none {
            ui.selectable_value(value, None, n);
        }
        for (k, l) in choices {
            ui.selectable_value(value, Some(k.clone()), l);
        }
    });
    *value != before
}

impl App {
    pub(super) fn per_app_panel(&mut self, ui: &mut Ui) {
        let s = self.s();
        widgets::card(ui, |ui| {
            let mut on = self.cfg.auto.enabled;
            if widgets::setting_row(ui, s.auto_title, s.auto_desc, &mut on, true) {
                self.cfg.auto.enabled = on;
                self.auto.restart();
                self.mark_dirty();
            }
            if !self.cfg.auto.enabled {
                return;
            }
            widgets::spaced_divider(ui, 8.0, 4.0);
            let choices = catalog::rule_choices(s, &self.cfg.custom_presets);
            self.default_rule(ui, &choices);
            let mut edits = self.rule_list(ui, &choices);
            widgets::spaced_divider(ui, 8.0, 10.0);
            edits.add = self.add_rule(ui);
            self.apply_rule_edits(edits, &choices);
        });
    }

    /// The preset for programs without a rule.
    fn default_rule(&mut self, ui: &mut Ui, choices: &[(String, String)]) {
        let s = self.s();
        let mut default = self.cfg.auto.default.clone();
        let changed = widgets::labeled_row(ui, s.auto_other, |ui| {
            preset_combo(ui, "auto-default", &mut default, choices, Some(s.auto_keep), s.missing_preset)
        });
        if changed {
            self.cfg.auto.default = default;
            self.mark_dirty();
        }
    }

    fn rule_list(&self, ui: &mut Ui, choices: &[(String, String)]) -> RuleEdits {
        let s = self.s();
        let mut edits = RuleEdits::default();
        for (i, rule) in self.cfg.auto.rules.iter().enumerate() {
            widgets::labeled_row(ui, &rule.exe, |ui| {
                if widgets::ghost(ui, s.delete).clicked() {
                    edits.remove = Some(i);
                }
                let mut v = Some(rule.preset.clone());
                if preset_combo(ui, ("auto-rule", i), &mut v, choices, None, s.missing_preset)
                    && let Some(v) = v
                {
                    edits.set = Some((i, v));
                }
            });
        }
        if self.cfg.auto.rules.is_empty() {
            ui.add_space(4.0);
            widgets::desc(ui, s.auto_empty);
        }
        edits
    }

    /// Recently used programs to pick from, and a field for any other.
    fn add_rule(&mut self, ui: &mut Ui) -> Option<String> {
        let s = self.s();
        let mut add = None;
        let recent = self.auto.recent_without_rule(&self.cfg.auto);
        if !recent.is_empty() {
            widgets::desc(ui, s.auto_recent);
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for e in &recent {
                    if widgets::ghost(ui, &format!("+  {e}")).clicked() {
                        add = Some(e.clone());
                    }
                }
            });
            ui.add_space(8.0);
        }
        exe_input(ui, &mut self.view.rule_input, s.auto_exe_hint, s.add).or(add)
    }

    fn apply_rule_edits(&mut self, edits: RuleEdits, choices: &[(String, String)]) {
        let rules = &mut self.cfg.auto.rules;
        let mut dirty = false;
        if let Some((i, v)) = edits.set {
            rules[i].preset = v;
            dirty = true;
        }
        if let Some(i) = edits.remove {
            rules.remove(i);
            dirty = true;
        }
        if let Some(exe) = edits.add
            && !rules.iter().any(|r| r.exe == exe)
        {
            let preset = choices.first().map(|c| c.0.clone()).unwrap_or_default();
            rules.push(AppRule { exe, preset });
            dirty = true;
        }
        if dirty {
            self.auto.rules_changed();
            self.mark_dirty();
        }
    }
}
