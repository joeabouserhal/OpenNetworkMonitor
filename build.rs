use std::{env, path::PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=app.manifest");
    println!("cargo::rerun-if-changed=build.rs");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "OpenNetworkMonitor's administrator manifest currently requires the Windows MSVC toolchain"
    );
    let manifest =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
            .join("app.manifest");
    assert!(manifest.is_file(), "Missing application manifest");

    // Scope these flags to the desktop binary. Unit tests live in the library,
    // and [[bin]] test=false keeps the test harness from inheriting elevation.
    println!("cargo::rustc-link-arg-bin=OpenNetworkMonitor=/MANIFEST:EMBED,ID=1");
    // Disable the linker's generated asInvoker fragment, then merge the explicit
    // requireAdministrator fragment supplied by app.manifest.
    println!("cargo::rustc-link-arg-bin=OpenNetworkMonitor=/MANIFESTUAC:NO");
    println!(
        "cargo::rustc-link-arg-bin=OpenNetworkMonitor=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
