//! Windows only: embeds `brand/cuboid.ico` as icon resource 1, and the version information, into
//! `cuboid-term.exe`, which is installed as `cuboid.com` (docs/design/l6_release.md 4). The icon is
//! committed; `cargo xtask package` renders it again from the logo masters.

fn main() {
    // First, so that no other host or target reruns the script after a change elsewhere in the package.
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    embed_resources();
}

#[cfg(windows)]
fn embed_resources() {
    // A Windows host that builds for another OS has nothing to embed.
    if !std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        return;
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let icon = std::path::Path::new(&manifest_dir).join("../cuboid-assets/brand/cuboid.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    winresource::WindowsResource::new()
        .set_icon(&icon.to_string_lossy())
        .set("ProductName", "cuboid")
        .set("FileDescription", "cuboid command line")
        .set("InternalName", "cuboid")
        .set("OriginalFilename", "cuboid.com")
        .compile_for("cuboid-term")
        .expect("cannot embed the icon: rc.exe of the Windows SDK is needed (README, build tools)");
}
