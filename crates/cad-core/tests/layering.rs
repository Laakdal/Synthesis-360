//! Enforces the crate layering (CADCraft-style): nothing below the application crate may depend
//! on a UI, windowing or GPU crate, so the CAD engine stays testable headless and the UI is
//! replaceable.
//!   cad-sketch  ->  cad-core  ->  cad-kernel  ->  cad-app (egui/eframe/wgpu)

const FORBIDDEN: &[&str] = &["egui", "eframe", "egui-wgpu", "egui_wgpu", "wgpu", "winit", "rfd", "egui_kittest"];

fn deps(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_deps = l.ends_with("dependencies]");
            continue;
        }
        if in_deps {
            if let Some((name, _)) = l.split_once('=') {
                out.push(name.trim().to_string());
            }
        }
    }
    out
}

#[test]
fn engine_crates_do_not_depend_on_ui_or_gpu() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for krate in ["cad-sketch", "cad-core", "cad-kernel"] {
        let text = std::fs::read_to_string(root.join(krate).join("Cargo.toml")).unwrap();
        for d in deps(&text) {
            assert!(!FORBIDDEN.contains(&d.as_str()), "{krate} must not depend on {d}");
        }
    }
}

#[test]
fn lower_layers_do_not_depend_on_higher_layers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let order = ["cad-sketch", "cad-core", "cad-kernel", "cad-app"];
    for (i, krate) in order.iter().enumerate() {
        let text = std::fs::read_to_string(root.join(krate).join("Cargo.toml")).unwrap();
        let d = deps(&text);
        for higher in &order[i + 1..] {
            assert!(!d.iter().any(|x| x == higher), "{krate} must not depend on {higher}");
        }
    }
}
