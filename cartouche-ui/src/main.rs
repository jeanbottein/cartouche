//! Cartouche retro UI — Rust + macroquad.
//!
//! M1: an app shell with tab navigation (Library / Status / Settings), reading
//! the real game library from `.cartouche/` via the bridge layer. The cover-art
//! Library is live; pipeline running (Status) and the Settings editor follow.

mod app;
mod bridge;
mod engine;

use app::App;
use macroquad::prelude::*;

fn window_conf() -> Conf {
    let mut conf = Conf {
        window_title: "Cartouche".to_owned(),
        window_width: 1280,
        window_height: 720,
        high_dpi: true,
        ..Default::default()
    };
    // Pace frames with vsync (typically 60fps; up to the panel's refresh rate).
    // A manual sleep-based limiter fights vsync and causes 40-50fps jitter, so we
    // let the GPU pace us. A configurable fps cap lands with the Settings screen.
    conf.platform.swap_interval = Some(1);
    conf
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut app = App::new();
    loop {
        app.frame();
        next_frame().await;
    }
}
