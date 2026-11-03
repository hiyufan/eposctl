//! Line icons drawn with the painter (no image assets), plus the app/tray icon bitmap.

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, vec2};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sound,
    Mic,
    Device,
    Settings,
}

fn arc(p: &Painter, c: Pos2, r: f32, a0: f32, a1: f32, s: Stroke) {
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    p.line(pts, s);
}

/// Draw `icon` centred in `rect` (designed on a 20×20 grid).
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let k = rect.width().min(rect.height()) / 20.0;
    let o = rect.center() - vec2(10.0, 10.0) * k;
    let at = |x: f32, y: f32| o + vec2(x, y) * k;
    let s = Stroke::new(1.6 * k.max(1.0), color);
    match icon {
        Icon::Sound => {
            // Three mixer faders.
            for (x, knob) in [(4.0, 12.5), (10.0, 6.5), (16.0, 10.5)] {
                p.line_segment([at(x, 3.0), at(x, 17.0)], s);
                p.circle_filled(at(x, knob), 2.4 * k, color);
            }
        }
        Icon::Mic => {
            p.rect_stroke(
                Rect::from_min_max(at(7.0, 2.0), at(13.0, 12.0)),
                3.0 * k,
                s,
                eframe::egui::StrokeKind::Middle,
            );
            arc(p, at(10.0, 9.0), 6.0 * k, 0.0, PI, s);
            p.line_segment([at(10.0, 15.0), at(10.0, 18.0)], s);
            p.line_segment([at(7.0, 18.0), at(13.0, 18.0)], s);
        }
        Icon::Device => {
            // Headset: band and two cups.
            arc(p, at(10.0, 11.0), 7.0 * k, PI, TAU, s);
            for x in [2.0, 14.0] {
                p.rect_stroke(
                    Rect::from_min_size(at(x, 10.5), vec2(4.0, 7.0) * k),
                    1.5 * k,
                    s,
                    eframe::egui::StrokeKind::Middle,
                );
            }
        }
        Icon::Settings => {
            // Gear: ring with eight teeth.
            let c = at(10.0, 10.0);
            p.circle_stroke(c, 5.0 * k, s);
            p.circle_stroke(c, 2.0 * k, s);
            for i in 0..8 {
                let a = i as f32 * TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 5.0 * k, c + d * 8.0 * k], Stroke::new(2.4 * k.max(1.0), color));
            }
        }
    }
}

/// RGBA bitmap: a ring whose bright arc shows the battery level (full ring when unknown),
/// with a headset glyph in the middle. Used for the tray and the window icon.
pub fn bitmap(size: u32, battery: Option<u8>, connected: bool) -> Vec<u8> {
    let n = size as f32;
    let c = n / 2.0;
    let outer = n * 0.48;
    let inner = n * 0.36;
    let accent = if connected { [0x44, 0xEA, 0xD6] } else { [0x8E, 0x90, 0x99] };
    let dim = [0x3A, 0x3D, 0x45];
    let frac = battery.map(|b| b as f32 / 100.0).unwrap_or(1.0);
    let mut px = vec![0u8; (size * size * 4) as usize];
    // 4×4 supersampling for smooth edges at 16 px.
    let ss = 4;
    for y in 0..size {
        for x in 0..size {
            let mut acc = [0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let fx = x as f32 + (sx as f32 + 0.5) / ss as f32 - c;
                    let fy = y as f32 + (sy as f32 + 0.5) / ss as f32 - c;
                    let d = (fx * fx + fy * fy).sqrt();
                    let col = if d <= outer && d >= inner {
                        // Angle clockwise from 12 o'clock, 0..1.
                        let a = (fx.atan2(-fy) / TAU).rem_euclid(1.0);
                        Some(if a <= frac { accent } else { dim })
                    } else if glyph(fx / inner, fy / inner) {
                        Some(accent)
                    } else {
                        None
                    };
                    if let Some(rgb) = col {
                        acc[0] += rgb[0] as f32;
                        acc[1] += rgb[1] as f32;
                        acc[2] += rgb[2] as f32;
                        acc[3] += 1.0;
                    }
                }
            }
            let i = ((y * size + x) * 4) as usize;
            if acc[3] > 0.0 {
                px[i] = (acc[0] / acc[3]) as u8;
                px[i + 1] = (acc[1] / acc[3]) as u8;
                px[i + 2] = (acc[2] / acc[3]) as u8;
                px[i + 3] = (acc[3] / (ss * ss) as f32 * 255.0) as u8;
            }
        }
    }
    px
}

/// Headset silhouette in unit coordinates (inner ring radius = 1).
fn glyph(x: f32, y: f32) -> bool {
    let r = (x * x + (y - 0.05).powi(2)).sqrt();
    let band = (0.50..=0.66).contains(&r) && y < 0.05;
    let cup = (0.38..=0.66).contains(&x.abs()) && (0.0..=0.48).contains(&y);
    band || cup
}
