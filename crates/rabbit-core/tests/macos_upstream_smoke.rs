//! Real-upstream smoke tests for the macOS install paths.
//!
//! These tests compile only on macOS and are gated behind `#[ignore]` so the
//! default `cargo test` invocation does not hit the network. CI runs them via
//! `cargo test -p rabbit-core --test macos_upstream_smoke -- --ignored` on the
//! macOS runner.
//!
//! The file is intentionally named without "install"/"setup" so the Windows
//! installer-detection heuristic does not trigger UAC elevation when cargo
//! tries to launch the (empty-on-Windows) test binary.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::PathBuf;

use rabbit_core::artifact::resolve_latest_artifacts;
use rabbit_core::model::{Architecture, Platform};
use rabbit_core::operation::{
    PackageOperationOptions, PackageOperationStatus, execute_resolved_package_operation,
};
use rabbit_core::package::{PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPER, PACKAGE_SWS};
use rabbit_core::receipt::load_install_state;
use rabbit_core::setup::{SetupOptions, execute_resolved_setup_operation};
use tempfile::tempdir;

fn smoke_setup_options(target_app_path: PathBuf, lock_path: PathBuf) -> SetupOptions {
    SetupOptions {
        dry_run: false,
        portable: true,
        allow_reaper_running: true,
        stage_unsupported: false,
        replace_osara_keymap: false,
        target_app_path: Some(target_app_path),
        lock_path: Some(lock_path),
        force_reinstall_packages: Vec::new(),
    }
}

fn smoke_package_options(lock_path: PathBuf) -> PackageOperationOptions {
    PackageOperationOptions {
        dry_run: false,
        allow_reaper_running: true,
        stage_unsupported: false,
        replace_osara_keymap: false,
        target_app_path: None,
        lock_path: Some(lock_path),
        force_reinstall_packages: Vec::new(),
    }
}

#[test]
#[ignore = "downloads the live ReaKontrol snapshot ZIP from GitHub"]
fn reakontrol_macos_real_install_extracts_dylib_into_user_plugins() {
    let dir = tempdir().unwrap();
    let resource_path = dir.path().join("REAPER");
    fs::create_dir_all(&resource_path).unwrap();
    let cache_dir = dir.path().join("cache");
    let lock_path = dir.path().join("install.lock");

    let artifacts = resolve_latest_artifacts(
        &[PACKAGE_REAKONTROL.to_string()],
        Platform::MacOs,
        Architecture::current(),
    )
    .expect("resolve ReaKontrol artifact");

    let report = execute_resolved_package_operation(
        &resource_path,
        artifacts,
        &cache_dir,
        &smoke_package_options(lock_path),
    )
    .expect("install ReaKontrol");

    assert_eq!(report.items.len(), 1);
    assert_eq!(
        report.items[0].status,
        PackageOperationStatus::InstalledOrChecked
    );
    let dylib = resource_path
        .join("UserPlugins")
        .join("reaper_kontrol.dylib");
    assert!(dylib.is_file(), "expected installed dylib at {dylib:?}");

    let state = load_install_state(&resource_path).unwrap().unwrap();
    assert!(state.packages.contains_key(PACKAGE_REAKONTROL));
}

#[test]
#[ignore = "downloads the live SWS macOS DMG from sws-extension.org"]
fn sws_macos_real_install_extracts_dylib_into_user_plugins() {
    let dir = tempdir().unwrap();
    let resource_path = dir.path().join("REAPER");
    fs::create_dir_all(&resource_path).unwrap();
    let cache_dir = dir.path().join("cache");
    let lock_path = dir.path().join("install.lock");

    let artifacts = resolve_latest_artifacts(
        &[PACKAGE_SWS.to_string()],
        Platform::MacOs,
        Architecture::current(),
    )
    .expect("resolve SWS artifact");

    let report = execute_resolved_package_operation(
        &resource_path,
        artifacts,
        &cache_dir,
        &smoke_package_options(lock_path),
    )
    .expect("install SWS");

    assert_eq!(report.items.len(), 1);
    assert_eq!(
        report.items[0].status,
        PackageOperationStatus::InstalledOrChecked
    );

    let user_plugins = resource_path.join("UserPlugins");
    let plugin_present = fs::read_dir(&user_plugins)
        .expect("read UserPlugins")
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("reaper_sws")
        });
    assert!(
        plugin_present,
        "expected a reaper_sws*.dylib under {user_plugins:?}"
    );

    let state = load_install_state(&resource_path).unwrap().unwrap();
    assert!(state.packages.contains_key(PACKAGE_SWS));
}

#[test]
#[ignore = "downloads the live OSARA macOS snapshot ZIP from GitHub"]
fn osara_macos_real_install_copies_assets_into_resource_path() {
    let dir = tempdir().unwrap();
    let resource_path = dir.path().join("REAPER");
    fs::create_dir_all(&resource_path).unwrap();
    let cache_dir = dir.path().join("cache");
    let lock_path = dir.path().join("install.lock");

    let artifacts = resolve_latest_artifacts(
        &[PACKAGE_OSARA.to_string()],
        Platform::MacOs,
        Architecture::current(),
    )
    .expect("resolve OSARA artifact");

    let report = execute_resolved_package_operation(
        &resource_path,
        artifacts,
        &cache_dir,
        &smoke_package_options(lock_path),
    )
    .expect("install OSARA");

    assert_eq!(report.items.len(), 1);
    assert_eq!(
        report.items[0].status,
        PackageOperationStatus::InstalledOrChecked
    );

    assert!(
        resource_path
            .join("UserPlugins")
            .join("reaper_osara.dylib")
            .is_file()
    );
    assert!(
        resource_path
            .join("KeyMaps")
            .join("OSARA.ReaperKeyMap")
            .is_file()
    );
    let locale_dir = resource_path.join("osara").join("locale");
    let has_locale = fs::read_dir(&locale_dir)
        .expect("read osara locale dir")
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name().to_string_lossy().ends_with(".po"));
    assert!(
        has_locale,
        "expected at least one .po locale file under {locale_dir:?}"
    );

    let state = load_install_state(&resource_path).unwrap().unwrap();
    assert!(state.packages.contains_key(PACKAGE_OSARA));
}

#[test]
#[ignore = "downloads the live REAPER universal DMG from reaper.fm"]
fn reaper_macos_real_install_copies_app_bundle_into_portable_target() {
    let dir = tempdir().unwrap();
    let resource_path = dir.path().join("PortableREAPER");
    let target_app_path = resource_path.join("REAPER.app");
    let cache_dir = dir.path().join("cache");
    let lock_path = dir.path().join("install.lock");

    let artifacts = resolve_latest_artifacts(
        &[PACKAGE_REAPER.to_string()],
        Platform::MacOs,
        Architecture::current(),
    )
    .expect("resolve REAPER artifact");

    let report = execute_resolved_setup_operation(
        &resource_path,
        artifacts,
        &cache_dir,
        &smoke_setup_options(target_app_path.clone(), lock_path),
    )
    .expect("install REAPER portable");

    let package_items = report.package_operation.items;
    assert_eq!(package_items.len(), 1);
    assert_eq!(
        package_items[0].status,
        PackageOperationStatus::InstalledOrChecked
    );

    assert!(
        target_app_path
            .join("Contents")
            .join("Info.plist")
            .is_file(),
        "expected REAPER.app/Contents/Info.plist at {target_app_path:?}"
    );
    assert!(
        resource_path.join("reaper.ini").is_file(),
        "expected portable reaper.ini under {resource_path:?}"
    );

    let state = load_install_state(&resource_path).unwrap().unwrap();
    assert!(state.packages.contains_key(PACKAGE_REAPER));
}
