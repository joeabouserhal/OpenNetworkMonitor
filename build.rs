use std::{env, fs, path::PathBuf};

#[path = "src/app_icon.rs"]
mod app_icon;

fn main() {
    println!("cargo::rerun-if-changed=app.manifest");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/app_icon.rs");

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

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let icon_path = output.join("OpenNetworkMonitor.ico");
    let mut icon = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16, 24, 32, 48, 64, 128, 256] {
        let image = ico::IconImage::from_rgba_data(size, size, app_icon::pixels(size));
        icon.add_entry(ico::IconDirEntry::encode_as_png(&image).expect("Encode application icon"));
    }
    icon.write(fs::File::create(&icon_path).expect("Create application icon"))
        .expect("Write application icon");
    let resource = output.join("app-icon.rc");
    let resource_path = icon_path
        .to_str()
        .expect("UTF-8 icon resource path")
        .replace('\\', "/")
        .replace('"', "\\\"");
    fs::write(&resource, format!("1 ICON \"{resource_path}\"\n"))
        .expect("Write icon resource definition");
    embed_resource::compile_for(&resource, ["OpenNetworkMonitor"], embed_resource::NONE)
        .manifest_required()
        .expect("Compile and embed executable icon");

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
