//! Reads CPU architecture out of an executable's file header.
//!
//! REAPER installs are decoupled from the host RABBIT runs on: an Apple Silicon
//! Mac can host an Intel REAPER under Rosetta, and Windows-on-ARM can host an
//! x86_64 REAPER. Stamping installs with `Architecture::current()` (the running
//! RABBIT's compile-time arch) silently mismatches the per-arch artifact picker
//! when those configurations come up. This module probes the actual binary so
//! detection can record what the targeted REAPER really is.
//!
//! The header parsing itself is delegated to the `object` crate (the same one
//! the Rust toolchain uses for `addr2line`, `cargo-binutils`, etc.) — we just
//! map its `Architecture` enum into ours and detect Mach-O fat / universal
//! binaries up front. Probe failures all collapse to `Architecture::Unknown`,
//! which the artifact resolver already treats as "fall back to the platform
//! default" so a corrupt or unreadable file can't regress existing behavior.

use std::fs;
use std::path::{Path, PathBuf};

use object::FileKind;

use crate::model::Architecture;

/// Probes the executable at `install_path`. On macOS the detection layer hands
/// us a `.app` bundle; we drill into `Contents/MacOS/` first. On Windows /
/// for plain files we use the path as given.
pub fn probe_executable_architecture(install_path: &Path) -> Architecture {
    let Some(executable) = resolve_executable_path(install_path) else {
        return Architecture::Unknown;
    };
    let Ok(data) = fs::read(&executable) else {
        return Architecture::Unknown;
    };
    architecture_from_bytes(&data)
}

fn architecture_from_bytes(data: &[u8]) -> Architecture {
    // Mach-O fat / universal binaries have to be checked before the generic
    // `object::File::parse` path: the artifact picker shortcuts them to
    // `Universal` (matching REAPER's `_universal.dmg` naming) and bypasses
    // the per-slice arch question entirely.
    match FileKind::parse(data) {
        Ok(FileKind::MachOFat32 | FileKind::MachOFat64) => return Architecture::Universal,
        Ok(_) => {}
        Err(_) => return Architecture::Unknown,
    }

    let Ok(file) = object::File::parse(data) else {
        return Architecture::Unknown;
    };
    match object::Object::architecture(&file) {
        object::Architecture::I386 => Architecture::X86,
        object::Architecture::X86_64 => Architecture::X64,
        // `object` collapses both `IMAGE_FILE_MACHINE_ARM64` and
        // `IMAGE_FILE_MACHINE_ARM64EC` into `Aarch64`. The artifact resolver
        // treats `Arm64` and `Arm64Ec` identically (both map to the same
        // arm64ec asset on Windows), so the lossy mapping is fine — we
        // canonicalise to `Arm64` and let the resolver decide.
        object::Architecture::Aarch64 => Architecture::Arm64,
        _ => Architecture::Unknown,
    }
}

fn resolve_executable_path(install_path: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(install_path).ok()?;
    if metadata.is_file() {
        return Some(install_path.to_path_buf());
    }

    // macOS .app bundle: read the executable from Contents/MacOS/. We don't
    // parse Info.plist for CFBundleExecutable — the directory always
    // contains exactly one file in practice (REAPER, Rabbit, etc.), and a
    // name-stem match against the bundle takes priority for robustness.
    let macos_dir = install_path.join("Contents").join("MacOS");
    let stem = install_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned);

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&macos_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_file = entry
                .file_type()
                .map(|file_type| file_type.is_file())
                .unwrap_or(false);
            if is_file {
                candidates.push(path);
            }
        }
    }

    if let Some(bundle_name) = stem.as_deref() {
        if let Some(matching) = candidates
            .iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(bundle_name))
        {
            return Some(matching.clone());
        }
    }
    candidates.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn probes_mach_o_fat_as_universal() {
        // `FileKind::parse` reads 16 bytes minimum to disambiguate the magic;
        // FAT_MAGIC + nfat_arch + zero-padding is enough for it to recognise
        // the file as MachOFat32.
        let dir = tempdir().unwrap();
        let path = dir.path().join("rabbit");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xca, 0xfe, 0xba, 0xbe]); // FAT_MAGIC (big-endian on disk)
        bytes.extend_from_slice(&0u32.to_be_bytes()); // nfat_arch = 0
        bytes.extend_from_slice(&[0u8; 8]); // pad to 16 bytes total
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            probe_executable_architecture(&path),
            Architecture::Universal
        );
    }

    #[test]
    fn probes_mach_o_thin_arm64() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rabbit");
        write_mach_o_thin_64(&path, MACHO_CPU_TYPE_ARM64);
        assert_eq!(probe_executable_architecture(&path), Architecture::Arm64);
    }

    #[test]
    fn probes_mach_o_thin_x86_64() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rabbit");
        write_mach_o_thin_64(&path, MACHO_CPU_TYPE_X86_64);
        assert_eq!(probe_executable_architecture(&path), Architecture::X64);
    }

    #[test]
    fn unknown_for_unrecognised_magic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("garbage.bin");
        std::fs::write(&path, b"not an executable header").unwrap();
        assert_eq!(probe_executable_architecture(&path), Architecture::Unknown);
    }

    #[test]
    fn unknown_for_missing_path() {
        let dir = tempdir().unwrap();
        assert_eq!(
            probe_executable_architecture(&dir.path().join("nope")),
            Architecture::Unknown
        );
    }

    #[test]
    fn drills_into_macos_app_bundle() {
        let dir = tempdir().unwrap();
        let bundle = dir.path().join("REAPER.app");
        let macos = bundle.join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        write_mach_o_thin_64(&macos.join("REAPER"), MACHO_CPU_TYPE_ARM64);
        assert_eq!(probe_executable_architecture(&bundle), Architecture::Arm64);
    }

    const MACHO_CPU_TYPE_X86_64: u32 = 0x0100_0007;
    const MACHO_CPU_TYPE_ARM64: u32 = 0x0100_000c;

    /// Writes a minimal 32-byte little-endian Mach-O 64-bit executable header
    /// — just enough for `object::File::parse` to identify the architecture.
    /// `ncmds` / `sizeofcmds` stay zero, which `object` accepts because we
    /// only ask it for `architecture()`, not load-command iteration.
    fn write_mach_o_thin_64(path: &Path, cputype: u32) {
        let mut file = File::create(path).unwrap();
        // MH_MAGIC_64 in little-endian (the modern Mach-O on-disk magic on
        // both Apple Silicon and Intel macOS).
        file.write_all(&0xfeed_facfu32.to_le_bytes()).unwrap();
        file.write_all(&cputype.to_le_bytes()).unwrap();
        file.write_all(&0u32.to_le_bytes()).unwrap(); // cpusubtype
        file.write_all(&2u32.to_le_bytes()).unwrap(); // filetype = MH_EXECUTE
        file.write_all(&0u32.to_le_bytes()).unwrap(); // ncmds
        file.write_all(&0u32.to_le_bytes()).unwrap(); // sizeofcmds
        file.write_all(&0u32.to_le_bytes()).unwrap(); // flags
        file.write_all(&0u32.to_le_bytes()).unwrap(); // reserved
    }
}
