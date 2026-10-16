#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! CAD360 — a native, Fusion 360-inspired parametric CAD application.
//!
//! Usage: `cad360 [design.c360] [--scenario steps.json [--screenshot-dir DIR]]`

mod app;
mod branding;
mod scenario;
mod state;
mod ui;
mod viewport;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut open: Option<PathBuf> = None;
    let mut scenario: Option<PathBuf> = None;
    let mut shots: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--scenario" => scenario = args.next().map(PathBuf::from),
            "--screenshot-dir" => shots = args.next().map(PathBuf::from),
            _ => open = Some(PathBuf::from(a)),
        }
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1600.0, 960.0]).with_min_inner_size([960.0, 600.0]).with_title("CAD360").with_icon(std::sync::Arc::new(branding::app_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "CAD360",
        opts,
        Box::new(move |cc| {
            let mut st = state::AppState::new();
            if let Some(p) = &open {
                st.open(p);
            }
            let mut app = app::CadApp::new(cc, st);
            if let Some(s) = &scenario {
                match scenario::Scenario::load(s, shots.clone()) {
                    Ok(sc) => app.scenario = Some(sc),
                    Err(e) => eprintln!("CAD360: could not load scenario {}: {e}", s.display()),
                }
            }
            Ok(Box::new(app))
        }),
    )
}
