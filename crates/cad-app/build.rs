//! Embeds the application icon and version information into cad360.exe on Windows
//! (Explorer, the taskbar before the window opens, and the file's Properties > Details tab).

fn main() {
    println!("cargo:rerun-if-changed=assets/cad360.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/cad360.ico")
        .set("ProductName", "CAD360")
        .set("FileDescription", "CAD360 parametric CAD")
        .set("InternalName", "cad360")
        .set("OriginalFilename", "cad360.exe")
        .set("ProductVersion", &version)
        .set("FileVersion", &version)
        .set("LegalCopyright", "CAD360 contributors. Open CASCADE Technology is LGPL-2.1 with exception.");
    if let Err(e) = res.compile() {
        // a missing Windows SDK resource compiler must not break the build: warn and go on
        println!("cargo:warning=CAD360: could not embed the Windows icon/version resource: {e}");
    }
}
