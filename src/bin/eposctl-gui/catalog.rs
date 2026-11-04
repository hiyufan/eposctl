//! The presets the GUI offers: factory and game presets from the library, then
//! the user's saved ones, and the ids that refer to them.

use eposctl::presets::{self, Preset};

use crate::config::{CustomPreset, Sound};
use crate::strings::Strings;

/// One preset in the list.
#[derive(Clone, Copy, Debug)]
pub enum PresetId {
    Factory(&'static Preset),
    /// Index into `Config::custom_presets`.
    Custom(usize),
}

impl PresetId {
    /// Tray menu id: the factory name, or `custom<index>`.
    pub fn key(self) -> String {
        match self {
            Self::Factory(p) => p.name.to_owned(),
            Self::Custom(i) => format!("custom{i}"),
        }
    }

    /// Inverse of `key`.
    pub fn from_key(key: &str) -> Option<Self> {
        if let Some(p) = presets::find(key) {
            return Some(Self::Factory(p));
        }
        key.strip_prefix("custom")?.parse().ok().map(Self::Custom)
    }

    /// Resolve a per-program rule's reference: a factory name or `custom:<name>`.
    /// Saved presets are referenced by name so deleting one does not shift the others.
    pub fn from_rule(rule: &str, custom: &[CustomPreset]) -> Option<Self> {
        match rule.strip_prefix(RULE_CUSTOM) {
            Some(name) => custom.iter().position(|p| p.name == name).map(Self::Custom),
            None => presets::find(rule).map(Self::Factory),
        }
    }

    /// The sound this preset sets, `None` for a saved preset that is gone.
    pub fn sound(self, custom: &[CustomPreset]) -> Option<Sound> {
        match self {
            Self::Factory(p) => Some(Sound::of_preset(p)),
            Self::Custom(i) => custom.get(i).map(|p| p.sound.clone()),
        }
    }
}

const RULE_CUSTOM: &str = "custom:";

/// Factory presets in the order the official app shows them, then the game presets.
pub fn factory() -> impl Iterator<Item = &'static Preset> {
    ["music", "esport", "movie", "flat"].into_iter().filter_map(presets::find).chain(presets::GAMES)
}

pub fn is_game(p: &Preset) -> bool {
    presets::GAMES.iter().any(|g| g.name == p.name)
}

pub fn factory_label(s: &Strings, name: &str) -> &'static str {
    match name {
        "music" => s.preset_music,
        "esport" => s.preset_esport,
        "movie" => s.preset_movie,
        "flat" => s.preset_flat,
        "valorant" => s.preset_valorant,
        "delta-force" => s.preset_delta_force,
        "cod" => s.preset_cod,
        "pubg" => s.preset_pubg,
        _ => s.preset_custom,
    }
}

/// Every preset in display order, with its label.
pub fn all(s: &Strings, custom: &[CustomPreset]) -> Vec<(PresetId, String)> {
    let factory = factory().map(|p| (PresetId::Factory(p), factory_label(s, p.name).to_owned()));
    let saved = custom.iter().enumerate().map(|(i, p)| (PresetId::Custom(i), p.name.clone()));
    factory.chain(saved).collect()
}

/// Presets for the tray menu, as (menu id, label).
pub fn tray_entries(s: &Strings, custom: &[CustomPreset]) -> Vec<(String, String)> {
    all(s, custom).into_iter().map(|(id, label)| (id.key(), label)).collect()
}

/// Presets a per-program rule can point at, as (rule reference, label).
pub fn rule_choices(s: &Strings, custom: &[CustomPreset]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> =
        factory().map(|p| (p.name.to_owned(), factory_label(s, p.name).to_owned())).collect();
    for p in custom {
        let key = format!("{RULE_CUSTOM}{}", p.name);
        if !v.iter().any(|(k, _)| *k == key) {
            v.push((key, p.name.clone()));
        }
    }
    v
}

/// The preset `dir` steps (1 = next, -1 = previous) away from the one that
/// sounds like `current`. From a sound that matches none, start at either end.
pub fn step(current: &Sound, custom: &[CustomPreset], dir: isize) -> Option<PresetId> {
    let ids: Vec<PresetId> = factory().map(PresetId::Factory).chain((0..custom.len()).map(PresetId::Custom)).collect();
    let n = ids.len() as isize;
    if n == 0 {
        return None;
    }
    let at = ids.iter().position(|id| id.sound(custom).is_some_and(|s| current.matches(&s)));
    let i = match at {
        Some(i) => (i as isize + dir).rem_euclid(n),
        None if dir > 0 => 0,
        None => n - 1,
    };
    Some(ids[i as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(name: &str) -> CustomPreset {
        CustomPreset { name: name.into(), sound: Sound::of_preset(presets::find("music").unwrap()) }
    }

    #[test]
    fn keys_round_trip() {
        for key in ["music", "custom3"] {
            assert_eq!(PresetId::from_key(key).map(PresetId::key).as_deref(), Some(key));
        }
        assert!(PresetId::from_key("nope").is_none());
    }

    #[test]
    fn rules_resolve_saved_presets_by_name() {
        let saved = [custom("A"), custom("B")];
        assert!(matches!(PresetId::from_rule("custom:B", &saved), Some(PresetId::Custom(1))));
        assert!(matches!(PresetId::from_rule("flat", &saved), Some(PresetId::Factory(p)) if p.name == "flat"));
        assert!(PresetId::from_rule("custom:C", &saved).is_none());
    }

    #[test]
    fn step_wraps_around() {
        let first = factory().next().unwrap();
        let last = step(&Sound::of_preset(first), &[], -1).unwrap();
        assert_eq!(step(&last.sound(&[]).unwrap(), &[], 1).unwrap().key(), first.name);
    }
}
