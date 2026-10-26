// The AppKit helpers in window-shim.m, compiled into the binary on macOS. Windows uses
// src/native_windows.rs instead.
fn main() {
    println!("cargo:rerun-if-changed=window-shim.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new().file("window-shim.m").flag("-fobjc-arc").compile("window_shim");
    println!("cargo:rustc-link-lib=framework=AppKit");
}
