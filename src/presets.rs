//! Factory sound presets.
//!
//! The official XML files only hold 5 EQ sliders while the APO has 9 bands; the
//! official UI expands them internally. The 9-band curves below are the exact
//! values the official UI wrote into the APO settings block (captured by diffing
//! the shared memory), not an interpolation of our own.

use crate::apo::{Settings, layout::BANDS};

#[derive(Debug)]
pub struct Preset {
    pub name: &'static str,
    pub speaker_eq: [f32; BANDS],
    /// 0..=1 (official slider value / 255)
    pub reverb: f32,
    /// 0..=1 (official slider value / 255); `None` leaves the microphone alone.
    pub noise_gate: Option<f32>,
    pub surround: bool,
}

impl Preset {
    /// Apply the speaker-side part of the preset. Microphone EQ is left alone
    /// until its relationship to presets is understood.
    pub fn apply(&self, s: &mut Settings) {
        s.set_speaker_eq(&self.speaker_eq);
        s.set_reverb(self.reverb);
        if let Some(g) = self.noise_gate {
            s.set_noise_gate(g);
        }
        s.set_surround(self.surround);
    }
}

pub const FACTORY: &[Preset] = &[
    Preset { name: "flat", speaker_eq: [0.0; BANDS], reverb: 0.0, noise_gate: Some(100.0 / 255.0), surround: false },
    Preset {
        name: "music",
        speaker_eq: [6.0, 3.97, 0.85, -0.43, -1.04, -0.47, 0.66, 4.02, 6.0],
        reverb: 0.0,
        noise_gate: Some(40.0 / 255.0),
        surround: false,
    },
    Preset {
        name: "esport",
        speaker_eq: [-6.0, -3.97, -0.85, 0.85, 2.03, 4.2, 6.0, 6.0, 6.0],
        reverb: 77.0 / 255.0,
        noise_gate: Some(90.0 / 255.0),
        surround: true,
    },
    Preset {
        name: "movie",
        speaker_eq: [6.0, 4.16, 1.28, 0.28, 0.0, 0.0, 0.66, 4.02, 6.0],
        reverb: 0.0,
        noise_gate: Some(170.0 / 255.0),
        surround: true,
    },
];

/// Footstep presets for particular games, tuned for the EPOS H3 series.
///
/// Nobody publishes measured footstep spectra for these games, so they are built
/// from the advice that recurs across community sources (cut the explosion and
/// vehicle low end, lift the 2-4 kHz scuff and click of footsteps), with
/// per-game differences where sources agree on them. On top of that, the
/// 1-4 kHz range gets about 1 dB more than the generic advice, because
/// SoundGuys measured it slightly recessed on the EPOS H3 Hybrid. Surround and
/// reverb are off: all four games do their own positional audio. Starting points,
/// to be adjusted by ear. Sources are listed in `docs/game-presets.md`.
pub const GAMES: &[Preset] = &[
    // Footsteps keep a low thud in VALORANT, so 125 Hz is lifted, not cut.
    Preset {
        name: "valorant",
        speaker_eq: [-1.5, 1.0, 0.0, -1.5, 0.0, 2.0, 3.0, 0.5, 0.0],
        reverb: 0.0,
        noise_gate: None,
        surround: false,
    },
    // Heavy vehicles and explosions: lows cut; reports of harsh gunfire: 8 kHz trimmed.
    Preset {
        name: "delta-force",
        speaker_eq: [-2.0, 0.0, 0.5, -1.0, 1.0, 3.0, 2.5, -0.5, 0.0],
        reverb: 0.0,
        noise_gate: None,
        surround: false,
    },
    // Strong low cut for explosions, main lift near 2.85 kHz.
    Preset {
        name: "cod",
        speaker_eq: [-3.5, -1.5, -1.5, -0.5, 0.5, 3.0, 3.5, 1.0, 0.0],
        reverb: 0.0,
        noise_gate: None,
        surround: false,
    },
    // Lift around 2 kHz, plus 4-8 kHz for footsteps on grass.
    Preset {
        name: "pubg",
        speaker_eq: [-3.0, -1.5, 0.0, 0.0, 1.0, 3.5, 2.5, 2.0, 0.0],
        reverb: 0.0,
        noise_gate: None,
        surround: false,
    },
];

/// Factory presets, then game presets.
pub fn all() -> impl Iterator<Item = &'static Preset> {
    FACTORY.iter().chain(GAMES)
}

pub fn find(name: &str) -> Option<&'static Preset> {
    all().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apo::{EQ_MAX_DB, EQ_MIN_DB};

    #[test]
    fn presets_are_in_range_and_unique() {
        let names: Vec<_> = all().map(|p| p.name).collect();
        for (i, p) in all().enumerate() {
            assert!(!names[..i].contains(&p.name), "duplicate {}", p.name);
            assert!(p.speaker_eq.iter().all(|g| (EQ_MIN_DB..=EQ_MAX_DB).contains(g)), "{}", p.name);
        }
        assert!(find("Delta-Force").is_some());
    }

    #[test]
    fn game_presets_leave_the_microphone_alone() {
        assert!(GAMES.iter().all(|p| p.noise_gate.is_none() && !p.surround));
    }
}
