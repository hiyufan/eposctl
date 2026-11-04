//! Debug builds only: environment variables used to take the README screenshots.
//!
//! - `EPOSCTL_PAGE=mic|device|settings|peq` opens on that page.
//! - `EPOSCTL_SCROLL=<px>` scrolls the page.
//! - `EPOSCTL_TEST_TOAST=1` shows a sample low-battery notification.
//! - `EPOSCTL_SHOT=file.png` saves a screenshot once the window has settled,
//!   then exits. `EPOSCTL_SHOT_DELAY` (seconds, default 3) and
//!   `EPOSCTL_SHOT_HEIGHT` (logical points from the top, default all) adjust it.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use eframe::egui;

use crate::notify;
use crate::strings::Strings;

fn env_f32(key: &str) -> Option<f32> {
    std::env::var(key).ok()?.parse().ok()
}

/// `EPOSCTL_PAGE`.
pub fn start_page() -> Option<String> {
    std::env::var("EPOSCTL_PAGE").ok()
}

/// `EPOSCTL_SCROLL`.
pub fn scroll() -> Option<f32> {
    env_f32("EPOSCTL_SCROLL")
}

/// `EPOSCTL_TEST_TOAST`. A failure is written to a file in the temp folder,
/// since a GUI build has no console.
pub fn sample_toast(s: &Strings) {
    if std::env::var_os("EPOSCTL_TEST_TOAST").is_none() {
        return;
    }
    let body = s.low_battery_body.replace("{name}", "EPOS H3PRO").replace("{pct}", "20");
    if let Err(e) = notify::toast(s.low_battery_title, &body) {
        let _ = std::fs::write(std::env::temp_dir().join("eposctl-toast-error.txt"), e.to_string());
    }
}

/// `EPOSCTL_SHOT`; call every frame.
pub fn screenshot(ctx: &egui::Context) {
    static START: OnceLock<Instant> = OnceLock::new();
    static ASKED: AtomicBool = AtomicBool::new(false);
    let Some(path) = std::env::var_os("EPOSCTL_SHOT") else { return };
    let start = *START.get_or_init(Instant::now);
    let delay = env_f32("EPOSCTL_SHOT_DELAY").unwrap_or(3.0);
    let height = env_f32("EPOSCTL_SHOT_HEIGHT").unwrap_or(f32::INFINITY);
    if start.elapsed().as_secs_f32() > delay && !ASKED.swap(true, Ordering::Relaxed) {
        let ppp = ctx.pixels_per_point();
        let screen = ctx.content_rect();
        let crop = egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, screen.max.y.min(height)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
            save_png(&path, &img.region(&crop, Some(ppp)));
            std::process::exit(0);
        })));
    }
    ctx.request_repaint();
}

fn save_png(path: &std::ffi::OsStr, img: &egui::ColorImage) {
    let file = std::fs::File::create(path).expect("screenshot file");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width() as u32, img.height() as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    enc.write_header().and_then(|mut w| w.write_image_data(&bytes)).expect("write png");
}
