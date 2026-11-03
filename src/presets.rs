//! Factory sound presets.
//!
//! The official XML files only hold 5 EQ sliders while the APO has 9 bands; the
//! official UI expands them internally. The 9-band curves below are the exact
//! values the official UI wrote into the APO settings block (captured by diffing
//! the shared memory), not an interpolation of our own.

use crate::apo::{Settings, layout::BANDS};

pub struct Preset {
    pub name: &'static str,
    pub speaker_eq: [f32; BANDS],
    /// 0..=1 (official slider value / 255)
    pub reverb: f32,
    /// 0..=1 (official slider value / 255)
    pub noise_gate: f32,
    pub surround: bool,
}

impl Preset {
    /// Apply the speaker-side part of the preset. Microphone EQ is left alone
    /// until its relationship to presets is understood.
    pub fn apply(&self, s: &mut Settings) {
        s.set_speaker_eq(&self.speaker_eq);
        s.set_reverb(self.reverb);
        s.set_noise_gate(self.noise_gate);
        s.set_surround(self.surround);
    }
}

pub const FACTORY: &[Preset] = &[
    Preset {
        name: "flat",
        speaker_eq: [0.0; BANDS],
        reverb: 0.0,
        noise_gate: 100.0 / 255.0,
        surround: false,
    },
    Preset {
        name: "music",
        speaker_eq: [6.0, 3.97, 0.85, -0.43, -1.04, -0.47, 0.66, 4.02, 6.0],
        reverb: 0.0,
        noise_gate: 40.0 / 255.0,
        surround: false,
    },
    Preset {
        name: "esport",
        speaker_eq: [-6.0, -3.97, -0.85, 0.85, 2.03, 4.2, 6.0, 6.0, 6.0],
        reverb: 77.0 / 255.0,
        noise_gate: 90.0 / 255.0,
        surround: true,
    },
    Preset {
        name: "movie",
        speaker_eq: [6.0, 4.16, 1.28, 0.28, 0.0, 0.0, 0.66, 4.02, 6.0],
        reverb: 0.0,
        noise_gate: 170.0 / 255.0,
        surround: true,
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    FACTORY.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}
