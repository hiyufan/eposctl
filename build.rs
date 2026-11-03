//! Embeds the icon and version info into the Windows executables, so Explorer
//! and Task Manager show the eposctl icon and name instead of a blank exe.

fn main() {
    println!("cargo:rerun-if-changed=assets/eposctl.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/eposctl.ico")
        .set("FileDescription", "eposctl: EPOS headset control")
        .set("ProductName", "eposctl")
        .set("LegalCopyright", "MIT License");
    // A missing resource compiler should not break the build; the exe just
    // has no icon then.
    if let Err(e) = res.compile() {
        println!("cargo:warning=no icon embedded: {e}");
    }
}
