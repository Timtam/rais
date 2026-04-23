fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        let manifest_path = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows")
            .join("rais-ui-wxdragon.exe.manifest");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
        println!("cargo:rustc-link-arg-bin=rais-ui-wxdragon=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=rais-ui-wxdragon=/MANIFESTINPUT:{}",
            manifest_path.display()
        );
    }
}
