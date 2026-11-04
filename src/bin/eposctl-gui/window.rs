//! Showing and hiding the main window (it lives on in the tray).

use eframe::egui::{self, ViewportCommand};

pub struct Window {
    /// We hid the window. Tracked here because the viewport's own visibility
    /// info does not reliably update after `Visible(false)`.
    hidden: bool,
    /// Frames left during which the window is kept hidden (`--minimized`).
    /// eframe shows the window after its first paint, so it is hidden again.
    hide_frames: u8,
}

impl Window {
    pub fn new(start_hidden: bool) -> Self {
        Self { hidden: false, hide_frames: if start_hidden { 3 } else { 0 } }
    }

    /// Call every frame; keeps a window started hidden from showing up.
    pub fn keep_hidden_at_start(&mut self, ctx: &egui::Context) {
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            self.hidden = true;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.hide_frames = 0;
        self.hidden = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Close to the tray instead of exiting.
    pub fn hide(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.hidden = true;
    }

    /// On screen: neither hidden nor minimized.
    pub fn is_shown(&self, ctx: &egui::Context) -> bool {
        !self.hidden && !ctx.input(|i| i.viewport().minimized.unwrap_or(false))
    }
}
