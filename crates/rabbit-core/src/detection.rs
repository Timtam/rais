use std::fs;
use std::path::{Path, PathBuf};

use crate::arch_probe::probe_executable_architecture;
use crate::error::{IoPathContext, Result};
use crate::metadata::file_version;
use crate::model::{
    ComponentDetection, Confidence, Evidence, Installation, InstallationKind, Platform,
};
use crate::package::{
    PACKAGE_FFMPEG, PACKAGE_JAWS_SCRIPTS, PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK,
    PACKAGE_SWS, PackageSpec, builtin_package_specs,
};
use crate::reapack::package_owner_for_file;
use crate::receipt::{ReceiptVerification, load_install_state, verify_package_receipt};

#[derive(Debug, Clone, Default)]
pub struct DiscoveryOptions {
    pub include_standard: bool,
    pub portable_roots: Vec<PathBuf>,
}

impl DiscoveryOptions {
    pub fn standard() -> Self {
        Self {
            include_standard: true,
            portable_roots: Vec::new(),
        }
    }
}

pub fn discover_installations(options: &DiscoveryOptions) -> Result<Vec<Installation>> {
    let Some(platform) = Platform::current() else {
        return Ok(Vec::new());
    };

    let mut installations = Vec::new();

    if options.include_standard {
        if let Some(standard) = discover_standard_installation(platform) {
            installations.push(standard);
        }
    }

    for portable_root in &options.portable_roots {
        if let Some(portable) = discover_portable_installation(platform, portable_root) {
            installations.push(portable);
        }
    }

    Ok(installations)
}

pub fn default_standard_installation(platform: Platform) -> Option<Installation> {
    match platform {
        Platform::Windows => standard_windows_installation(false),
        Platform::MacOs => standard_macos_installation(false),
    }
}

pub fn detect_components(
    resource_path: &Path,
    platform: Platform,
) -> Result<Vec<ComponentDetection>> {
    detect_components_with_probes(
        resource_path,
        platform,
        rabbit_platform::read_uninstall_display_version,
    )
}

pub(crate) fn detect_components_with_probes(
    resource_path: &Path,
    platform: Platform,
    uninstall_display_version: fn(&str) -> Option<String>,
) -> Result<Vec<ComponentDetection>> {
    let state = load_install_state(resource_path)?;
    let mut detections = Vec::new();

    for spec in builtin_package_specs(platform) {
        detections.push(detect_component_with_probes(
            resource_path,
            platform,
            &spec,
            state.as_ref(),
            uninstall_display_version,
        )?);
    }

    Ok(detections)
}

pub fn detect_component(
    resource_path: &Path,
    platform: Platform,
    spec: &PackageSpec,
    state: Option<&crate::receipt::InstallState>,
) -> Result<ComponentDetection> {
    detect_component_with_probes(
        resource_path,
        platform,
        spec,
        state,
        rabbit_platform::read_uninstall_display_version,
    )
}

pub(crate) fn detect_component_with_probes(
    resource_path: &Path,
    platform: Platform,
    spec: &PackageSpec,
    state: Option<&crate::receipt::InstallState>,
    uninstall_display_version: fn(&str) -> Option<String>,
) -> Result<ComponentDetection> {
    match verify_package_receipt(resource_path, state, &spec.id)? {
        ReceiptVerification::Verified(receipt) => {
            let files = receipt
                .installed_files
                .iter()
                .map(|file| resource_path.join(&file.path))
                .collect();
            return Ok(ComponentDetection {
                package_id: spec.id.clone(),
                display_name: spec.display_name.clone(),
                installed: true,
                version: receipt.version,
                detector: "rabbit-receipt".to_string(),
                confidence: Confidence::High,
                files,
                notes: Vec::new(),
            });
        }
        ReceiptVerification::Mismatch(receipt) => {
            let files = matching_user_plugin_files(resource_path, platform, spec)?;
            if !files.is_empty() {
                return Ok(ComponentDetection {
                    package_id: spec.id.clone(),
                    display_name: spec.display_name.clone(),
                    installed: true,
                    version: receipt.version,
                    detector: "rabbit-receipt-mismatch".to_string(),
                    confidence: Confidence::Medium,
                    files,
                    notes: vec![
                        "RABBIT has a receipt for this package, but installed files do not match it."
                            .to_string(),
                    ],
                });
            }
        }
        ReceiptVerification::MissingReceipt | ReceiptVerification::MissingPackage => {}
    }

    let files = matching_user_plugin_files(resource_path, platform, spec)?;
    if files.is_empty() {
        // JAWS-for-REAPER scripts don't drop anything under
        // `<resource>/UserPlugins` that we can match on prefix/suffix (the
        // ComAccess DLL is the only UserPlugins file and its name does not
        // share a stable prefix with the package id). So when the receipt
        // and per-file probes don't apply, fall through to the dedicated
        // registry/Uninstall.exe probe before giving up.
        if spec.id == PACKAGE_JAWS_SCRIPTS {
            if let Some(detection) = detect_jaws_scripts_via_uninstall_exe(spec) {
                return Ok(detection);
            }
        }
        return Ok(ComponentDetection::not_installed(
            spec.id.clone(),
            spec.display_name.clone(),
        ));
    }

    if let Some((version, detector, confidence, notes)) = detect_version_from_files_with_probes(
        resource_path,
        &files,
        &spec.id,
        uninstall_display_version,
    )? {
        return Ok(ComponentDetection {
            package_id: spec.id.clone(),
            display_name: spec.display_name.clone(),
            installed: true,
            version: Some(version),
            detector,
            confidence,
            files,
            notes,
        });
    }

    Ok(ComponentDetection {
        package_id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        installed: true,
        version: None,
        detector: "userplugins-file-presence".to_string(),
        confidence: Confidence::Medium,
        files,
        notes: vec!["Package is present, but this RABBIT version cannot reliably read its version without a RABBIT receipt.".to_string()],
    })
}

fn detect_version_from_files_with_probes(
    resource_path: &Path,
    files: &[PathBuf],
    package_id: &str,
    uninstall_display_version: fn(&str) -> Option<String>,
) -> Result<Option<(crate::version::Version, String, Confidence, Vec<String>)>> {
    // FFmpeg: the libavformat / libavcodec / etc. DLLs carry their
    // *library* major (62.3.100 for libavformat 62) in VS_FIXEDFILEINFO,
    // not the FFmpeg release version, so the generic file-version probe
    // below would mis-report. We dispatch to a custom resolver that:
    //   1. tries `ffmpeg.exe` / `ffprobe.exe` / `ffplay.exe`
    //      VS_FIXEDFILEINFO — these carry the real release on Gyan and
    //      tordona builds, e.g. `8.1.1.0` for FFmpeg 8.1.1; and
    //   2. falls back to a filename-based libavformat-major →
    //      FFmpeg-major mapping (`avformat-62.dll` → `8.0.0`) when no
    //      executable is present or its VS_FIXEDFILEINFO is the
    //      uninformative `0.0.0.0` placeholder.
    if package_id == PACKAGE_FFMPEG {
        if let Some(detection) = detect_ffmpeg_version(files) {
            return Ok(Some(detection));
        }
        return Ok(None);
    }

    // OSARA: Windows installers register a `DisplayVersion` under the standard
    // Uninstall key. Prefer that for non-RABBIT-managed OSARA installs because
    // it reflects what the user sees in Programs and Features.
    if package_id == PACKAGE_OSARA {
        if let Some(value) = uninstall_display_version("OSARA") {
            if let Ok(version) = crate::version::Version::parse(&value) {
                return Ok(Some((
                    version,
                    "windows-uninstall-displayversion".to_string(),
                    Confidence::High,
                    vec![format!(
                        "Version came from the OSARA Windows installer's Uninstall registry key."
                    )],
                )));
            }
        }
    }

    // SWS / ReaPack: when the file is registered in ReaPack's local registry
    // database, treat that as authoritative — it reflects what ReaPack thinks
    // is installed for users who installed the package via ReaPack rather
    // than the standalone vendor installer.
    if matches!(package_id, PACKAGE_SWS | PACKAGE_REAPACK) {
        for file in files {
            if let Some(owner) = package_owner_for_file(resource_path, file)? {
                return Ok(Some((
                    owner.version,
                    "reapack-registry".to_string(),
                    Confidence::High,
                    vec![format!(
                        "Version came from ReaPack registry entry {}/{}/{}.",
                        owner.remote, owner.category, owner.package
                    )],
                )));
            }
        }
    }

    for file in files {
        if let Some(version) = file_version(file)? {
            return Ok(Some((
                version,
                "file-version-metadata".to_string(),
                Confidence::High,
                Vec::new(),
            )));
        }
    }

    for file in files {
        if let Some(owner) = package_owner_for_file(resource_path, file)? {
            return Ok(Some((
                owner.version,
                "reapack-registry".to_string(),
                Confidence::High,
                vec![format!(
                    "Version came from ReaPack registry entry {}/{}/{}.",
                    owner.remote, owner.category, owner.package
                )],
            )));
        }
    }

    if package_id == PACKAGE_OSARA {
        for file in files {
            if let Some(version) = embedded_snapshot_version_from_binary(file)? {
                return Ok(Some((
                    version,
                    "osara-binary-version-string".to_string(),
                    Confidence::Medium,
                    vec![
                        "Version came from a best-effort scan for OSARA's embedded version string."
                            .to_string(),
                    ],
                )));
            }
        }
    }

    if package_id == PACKAGE_REAKONTROL {
        for file in files {
            if let Some(version) = embedded_snapshot_version_from_binary(file)? {
                return Ok(Some((
                    version,
                    "reakontrol-binary-version-string".to_string(),
                    Confidence::Medium,
                    vec![
                        "Version came from a best-effort scan for ReaKontrol's embedded version string."
                            .to_string(),
                    ],
                )));
            }
        }
    }

    if package_id == PACKAGE_SWS {
        for file in files {
            if let Some(version) = sws_version_from_binary(file)? {
                return Ok(Some((
                    version,
                    "sws-binary-version-string".to_string(),
                    Confidence::Medium,
                    vec![
                        "Version came from a best-effort scan for SWS's embedded `version #commit` string."
                            .to_string(),
                    ],
                )));
            }
        }
    }

    if package_id == PACKAGE_REAPACK {
        for file in files {
            if let Some(version) = reapack_version_from_binary(file)? {
                return Ok(Some((
                    version,
                    "reapack-binary-version-string".to_string(),
                    Confidence::Medium,
                    vec![
                        "Version came from a best-effort scan for ReaPack's embedded user-agent string."
                            .to_string(),
                    ],
                )));
            }
        }
    }

    Ok(None)
}

fn embedded_snapshot_version_from_binary(path: &Path) -> Result<Option<crate::version::Version>> {
    let bytes = fs::read(path).with_path(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(embedded_snapshot_version_from_text(&text))
}

fn sws_version_from_binary(path: &Path) -> Result<Option<crate::version::Version>> {
    let bytes = fs::read(path).with_path(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(sws_version_from_text(&text))
}

/// Look for SWS's distinctive `<version> #<git-hash>` literal — embedded in
/// the about-dialog and user-agent strings (e.g., `2.14.0.1 #2dadf4b`). The
/// trailing space-hash-hex anchor is what makes this safe to grep without
/// false positives on arbitrary digit clusters in the binary.
fn sws_version_from_text(text: &str) -> Option<crate::version::Version> {
    let bytes = text.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }

        let mut end = start;
        let mut dot_count = 0;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            if bytes[end] == b'.' {
                dot_count += 1;
            }
            end += 1;
        }

        // SWS releases are at least three-component (e.g., 2.14.0); accept
        // both 3- and 4-component forms.
        if dot_count < 2 || bytes.get(end..end + 2) != Some(b" #") {
            start += 1;
            continue;
        }

        let mut hash_end = end + 2;
        while hash_end < bytes.len() && bytes[hash_end].is_ascii_hexdigit() {
            hash_end += 1;
        }
        if hash_end - (end + 2) < 6 {
            start += 1;
            continue;
        }

        let candidate = &text[start..end];
        if let Ok(version) = crate::version::Version::parse(candidate) {
            return Some(version);
        }
        start += 1;
    }

    None
}

fn reapack_version_from_binary(path: &Path) -> Result<Option<crate::version::Version>> {
    let bytes = fs::read(path).with_path(path)?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(reapack_version_from_text(&text))
}

/// Look for ReaPack's distinctive `ReaPack/<version>` user-agent literal (or
/// the legacy `ReaPack v<version>` form some builds embed in the about
/// dialog). The "ReaPack" prefix is unique enough that the version digits
/// that follow are reliably ReaPack's own.
fn reapack_version_from_text(text: &str) -> Option<crate::version::Version> {
    for prefix in ["ReaPack/", "ReaPack v"] {
        let mut cursor = 0;
        while cursor < text.len() {
            let Some(idx) = text[cursor..].find(prefix) else {
                break;
            };
            let after = &text[cursor + idx + prefix.len()..];
            let end = after
                .as_bytes()
                .iter()
                .position(|byte| !(byte.is_ascii_digit() || *byte == b'.'))
                .unwrap_or(after.len());
            let candidate = after[..end].trim_end_matches('.');
            if !candidate.is_empty()
                && candidate.contains('.')
                && let Ok(version) = crate::version::Version::parse(candidate)
            {
                return Some(version);
            }
            cursor += idx + prefix.len();
        }
    }

    None
}

fn embedded_snapshot_version_from_text(text: &str) -> Option<crate::version::Version> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].is_ascii_digit() {
            continue;
        }

        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'.' | b'-'))
        {
            end += 1;
        }

        let candidate = &text[start..end];
        if !candidate.starts_with("20") || candidate.matches('.').count() < 2 {
            continue;
        }

        if let Ok(version) = crate::version::Version::parse(candidate) {
            return Some(version);
        }
    }

    None
}

/// FFmpeg version detector that tries three probes in order:
///
/// 1. **`ffmpeg.exe` `ProductVersion` StringFileInfo** — vanilla
///    FFmpeg builds attach only a manifest to the binaries, so this
///    field is usually absent; but if a vendor (winget MSI wrapper,
///    custom build) patches it in, we read it for free without a
///    process spawn. `FileVersion` carries `LIBAVUTIL_VERSION` and
///    isn't useful here (libavutil 60.x.y for FFmpeg 8.x).
/// 2. **Binary string scan of `ffmpeg.exe`** — vanilla FFmpeg's
///    `show_banner` format string is a single contiguous literal of
///    the shape `"%s version <FFMPEG_VERSION>, Copyright (c) %d-%d the
///    FFmpeg developers\n"`, with `FFMPEG_VERSION` substituted at
///    compile time. We anchor on the distinctive `the FFmpeg
///    developers` suffix, then read backwards to pick out the version
///    token between `version ` and `,`. This replaces the previous
///    `ffmpeg.exe -version` subprocess spawn, which on Windows blocked
///    the UI thread for ~30 s while AV scanned FFmpeg's dozens of
///    DLL dependencies on every launch. The scan is two `memmem`
///    searches over a ~100 KB read — well below 1 ms in practice.
/// 3. **libavformat-major filename heuristic** — fallback when no
///    executable is present or the binary scan can't find the
///    anchor. Maps `avformat-XX.dll`'s libavformat major to the
///    FFmpeg release major (lib 58→FFmpeg 4, 59→5, 60→6, 61→7,
///    62→8, i.e. `lib_major - 54`). Patch level isn't recoverable
///    from the filename alone, so we synthesize as `<major>.0.0` at
///    `Medium` confidence.
fn detect_ffmpeg_version(
    files: &[PathBuf],
) -> Option<(crate::version::Version, String, Confidence, Vec<String>)> {
    // Walk `ffmpeg.exe` / `ffprobe.exe` / `ffplay.exe`. Any matched
    // file's parent points at the UserPlugins directory the
    // executables live in.
    let candidates: Vec<PathBuf> = files
        .iter()
        .find_map(|file| file.parent())
        .map(|parent| {
            ["ffmpeg.exe", "ffprobe.exe", "ffplay.exe"]
                .iter()
                .map(|name| parent.join(name))
                .filter(|path| path.is_file())
                .collect()
        })
        .unwrap_or_default();

    // Probe 1: ProductVersion (cheap, no process spawn). Almost always
    // absent on vanilla FFmpeg — kept so a future vendor that patches
    // VERSIONINFO works without a spawn.
    for exe_path in &candidates {
        let Some(raw) = rabbit_platform::read_string_file_info_key(exe_path, "ProductVersion")
        else {
            continue;
        };
        let Some(version) = ffmpeg_version_from_product_version_string(&raw) else {
            continue;
        };
        let major = version.numeric_parts().first().copied().unwrap_or(0);
        if (4..=9).contains(&major) {
            let exe_name = exe_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ffmpeg.exe")
                .to_string();
            return Some((
                version,
                "ffmpeg-productversion".to_string(),
                Confidence::High,
                vec![format!(
                    "Version came from {exe_name}'s `ProductVersion` StringFileInfo entry ({raw:?})."
                )],
            ));
        }
    }

    // Probe 2: binary string scan of `ffmpeg.exe`. The vanilla FFmpeg
    // build's `show_banner` format string embeds `FFMPEG_VERSION`
    // verbatim, so the literal version sits inside one contiguous
    // string in `.rdata` we can grep out without touching the
    // executable as a process. We invoke at most one of the three
    // candidates (ffmpeg, ffprobe, ffplay) — they all share the same
    // fftools cmdutils path, so the first hit is authoritative.
    for exe_path in &candidates {
        let Ok(bytes) = fs::read(exe_path) else {
            continue;
        };
        let Some(version) = ffmpeg_version_from_binary_bytes(&bytes) else {
            continue;
        };
        let major = version.numeric_parts().first().copied().unwrap_or(0);
        if (4..=9).contains(&major) {
            let exe_name = exe_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("ffmpeg.exe")
                .to_string();
            return Some((
                version,
                "ffmpeg-binary-version-string".to_string(),
                Confidence::High,
                vec![format!(
                    "Version came from a binary scan of {exe_name} for FFmpeg's `show_banner` format string."
                )],
            ));
        }
    }

    // Probe 3: libavformat-major filename heuristic.
    for file in files {
        let basename = file.file_name().and_then(|name| name.to_str())?;
        if let Some(lib_major) = libavformat_major_from_basename(basename) {
            let ffmpeg_major = ffmpeg_major_from_libavformat_major(lib_major)?;
            let version = crate::version::Version::parse(format!("{ffmpeg_major}.0.0")).ok()?;
            return Some((
                version,
                "ffmpeg-libavformat-major".to_string(),
                Confidence::Medium,
                vec![format!(
                    "Mapped {basename}'s libavformat major version to the corresponding FFmpeg release major. The patch level isn't recoverable from the DLL filename alone, so the detected version is reported as `<major>.0.0`."
                )],
            ));
        }
    }
    None
}

/// Scan a vanilla FFmpeg binary for the `show_banner` format-string
/// literal and pull the embedded `FFMPEG_VERSION` out of it. The
/// banner is one contiguous string. Upstream FFmpeg uses:
///
/// ```text
/// %s version <VERSION>, Copyright (c) <year>-<year> the FFmpeg developers
/// ```
///
/// — but some redistributors patch the format. Gyan.dev's full builds,
/// for example, drop the comma and pad the version with trailing
/// spaces (`%s version 8.1.1-full_build-www.gyan.dev         Copyright
/// (c) …`), and tordona's ARM snapshots embed `n8.1.1-6-…` directly.
/// We anchor on the distinctive `the FFmpeg developers` suffix, walk
/// back to find the trailing `version ` token, and treat the *next*
/// `Copyright` (with any preceding comma/whitespace stripped) as the
/// terminator. That matches both the upstream and the Gyan variant
/// without enumerating every redistributor's exact format.
///
/// `<VERSION>` is whatever `FFMPEG_VERSION` was `#define`d to at
/// compile time — `8.1.1` for Gyan stable, `n8.1.1` / `n8.1.1-6-…`
/// for snapshot / git builds — which we parse with the same helper
/// that handles `ProductVersion`. Returns `None` when the anchor
/// can't be found (non-FFmpeg binary, stripped strings table, …).
fn ffmpeg_version_from_binary_bytes(bytes: &[u8]) -> Option<crate::version::Version> {
    const ANCHOR: &[u8] = b" the FFmpeg developers";
    let anchor_pos = bytes.windows(ANCHOR.len()).position(|w| w == ANCHOR)?;
    // The format string is one contiguous run; cap the look-back at
    // 256 bytes so a stripped-string-table binary that happens to have
    // the anchor in an unrelated context can't drag us through MB of
    // bytes hunting for `version `.
    let start_limit = anchor_pos.saturating_sub(256);
    let prelude = &bytes[start_limit..anchor_pos];
    let prelude_str = std::str::from_utf8(prelude).ok()?;
    let version_marker = "version ";
    let version_pos = prelude_str.rfind(version_marker)?;
    let after = &prelude_str[version_pos + version_marker.len()..];
    let copyright_pos = after.find("Copyright")?;
    let version_str = after[..copyright_pos]
        .trim_end_matches(|ch: char| ch.is_whitespace() || ch == ',');
    ffmpeg_version_from_product_version_string(version_str)
}

/// Pull the leading version-shaped run out of FFmpeg's
/// `ProductVersion` string. Accepts forms like:
///
/// - `8.1.1` (Gyan stable)
/// - `n8.1.1` (BtbN-style tag)
/// - `n8.1.1-6-gdeadbeef-tordona-ffmpeg-builds-binaries` (tordona)
/// - `8.1.1-6-gdeadbeef`
///
/// Strips an optional `n` / `v` prefix, takes the longest leading
/// digit-or-dot run, and parses it as a `Version`.
fn ffmpeg_version_from_product_version_string(raw: &str) -> Option<crate::version::Version> {
    let stripped = raw
        .trim()
        .trim_start_matches(|ch: char| matches!(ch, 'n' | 'N' | 'v' | 'V'));
    let mut end = 0;
    for (idx, ch) in stripped.char_indices() {
        if ch.is_ascii_digit() || ch == '.' {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let candidate = stripped[..end].trim_end_matches('.');
    crate::version::Version::parse(candidate).ok()
}

/// Parse the libavformat major from `avformat-<MAJOR>.dll` (Windows) or
/// `libavformat.<MAJOR>.dylib` (macOS) filenames. Other filenames
/// return `None`.
fn libavformat_major_from_basename(basename: &str) -> Option<u64> {
    let lower = basename.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("avformat-") {
        let stem = rest.strip_suffix(".dll")?;
        return stem.parse().ok();
    }
    if let Some(rest) = lower.strip_prefix("libavformat.") {
        let stem = rest.strip_suffix(".dylib")?;
        return stem.parse().ok();
    }
    None
}

fn ffmpeg_major_from_libavformat_major(libavformat_major: u64) -> Option<u64> {
    // libavformat 58 → FFmpeg 4; each subsequent libavformat major
    // bump corresponds to the next FFmpeg major. Computed rather than
    // table-lookup so a future FFmpeg release that follows the same
    // pattern keeps working without a code change.
    libavformat_major.checked_sub(54)
}

pub(crate) fn matching_user_plugin_files(
    resource_path: &Path,
    _platform: Platform,
    spec: &PackageSpec,
) -> Result<Vec<PathBuf>> {
    let user_plugins = resource_path.join("UserPlugins");
    if !user_plugins.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(&user_plugins).with_path(&user_plugins)? {
        let entry = entry.with_path(&user_plugins)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower_name = file_name.to_ascii_lowercase();

        let prefix_matches = spec
            .user_plugin_prefixes
            .iter()
            .any(|prefix| lower_name.starts_with(&prefix.to_ascii_lowercase()));
        let suffix_matches = spec
            .user_plugin_suffixes
            .iter()
            .any(|suffix| lower_name.ends_with(&suffix.to_ascii_lowercase()));

        if prefix_matches && suffix_matches {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

fn discover_standard_installation(platform: Platform) -> Option<Installation> {
    match platform {
        Platform::Windows => standard_windows_installation(true),
        Platform::MacOs => standard_macos_installation(true),
    }
}

fn discover_portable_installation(platform: Platform, root: &Path) -> Option<Installation> {
    match platform {
        Platform::Windows => discover_portable_windows(root),
        Platform::MacOs => discover_portable_macos(root),
    }
}

fn standard_windows_installation(require_existing: bool) -> Option<Installation> {
    let resource_path = rabbit_platform::user_appdata_dir().map(|path| path.join("REAPER"))?;

    let app_path = windows_reaper_app_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\REAPER\reaper.exe"));

    if require_existing && !app_path.exists() && !resource_path.exists() {
        return None;
    }

    let mut evidence = Vec::new();
    if app_path.exists() {
        evidence.push(Evidence::new(
            "standard-windows-app-path",
            Some(app_path.clone()),
            "Found reaper.exe in a standard application directory.",
        ));
    }
    if resource_path.exists() {
        evidence.push(Evidence::new(
            "standard-windows-resource-path",
            Some(resource_path.clone()),
            "Found the standard REAPER resource path.",
        ));
    }

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "standard-windows-file-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from executable metadata."),
        ));
    }

    let probed_architecture = probe_executable_architecture(&app_path);
    Some(Installation {
        kind: InstallationKind::Standard,
        platform: Platform::Windows,
        app_path,
        resource_path: resource_path.clone(),
        version,
        architecture: Some(probed_architecture),
        writable: is_probably_writable(&resource_path),
        confidence: if !require_existing && evidence.is_empty() {
            Confidence::Low
        } else if evidence.len() > 1 {
            Confidence::High
        } else {
            Confidence::Medium
        },
        evidence,
    })
}

fn windows_reaper_app_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Prefer the install path the REAPER uninstaller wrote to the registry.
    // This catches non-default install dirs the user may have picked, plus the
    // default 64-bit location `C:\Program Files\REAPER (x64)\` which the
    // hardcoded `Program Files\REAPER\` fallback below misses.
    for key in ["REAPER", "REAPER_x64", "REAPER (x64)", "REAPER (x86_64)"] {
        if let Some(install_location) = rabbit_platform::read_uninstall_install_location(key) {
            let trimmed = install_location.trim().trim_end_matches(['\\', '/']);
            if !trimmed.is_empty() {
                let candidate = PathBuf::from(trimmed).join("reaper.exe");
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }

    // Also walk the standard Program Files dirs for both the plain `REAPER`
    // subfolder and the `REAPER (x64)` variant the 64-bit installer uses by
    // default. Order matters: registry hits win, then 64-bit-named variants,
    // then the bare folder name.
    for program_files in rabbit_platform::windows_program_files_dirs() {
        for subdir in ["REAPER (x64)", "REAPER (x86_64)", "REAPER"] {
            let candidate = program_files.join(subdir).join("reaper.exe");
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates
}

fn discover_portable_windows(root: &Path) -> Option<Installation> {
    let app_path = root.join("reaper.exe");
    let ini_path = root.join("reaper.ini");
    if !app_path.is_file() || !ini_path.is_file() {
        return None;
    }

    let version = file_version(&app_path).ok().flatten();
    let mut evidence = vec![
        Evidence::new(
            "portable-windows-app-path",
            Some(app_path.clone()),
            "Found reaper.exe in the selected portable folder.",
        ),
        Evidence::new(
            "portable-windows-reaper-ini",
            Some(ini_path),
            "Found reaper.ini in the selected portable folder.",
        ),
    ];
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "portable-windows-file-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from executable metadata."),
        ));
    }

    let probed_architecture = probe_executable_architecture(&app_path);
    Some(Installation {
        kind: InstallationKind::Portable,
        platform: Platform::Windows,
        app_path: app_path.clone(),
        resource_path: root.to_path_buf(),
        version,
        architecture: Some(probed_architecture),
        writable: is_probably_writable(root),
        confidence: Confidence::High,
        evidence,
    })
}

fn standard_macos_installation(require_existing: bool) -> Option<Installation> {
    let home = rabbit_platform::user_home_dir()?;
    let resource_path = home
        .join("Library")
        .join("Application Support")
        .join("REAPER");
    let app_path = [
        "/Applications/REAPER.app",
        "/Applications/REAPER64.app",
        "/Applications/REAPER-ARM.app",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.exists())
    .unwrap_or_else(|| PathBuf::from("/Applications/REAPER.app"));

    if require_existing && !app_path.exists() && !resource_path.exists() {
        return None;
    }

    let mut evidence = Vec::new();
    if app_path.exists() {
        evidence.push(Evidence::new(
            "standard-macos-app-path",
            Some(app_path.clone()),
            "Found REAPER.app in /Applications.",
        ));
    }
    if resource_path.exists() {
        evidence.push(Evidence::new(
            "standard-macos-resource-path",
            Some(resource_path.clone()),
            "Found the standard REAPER resource path.",
        ));
    }

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "standard-macos-app-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from app metadata."),
        ));
    }

    let probed_architecture = probe_executable_architecture(&app_path);
    Some(Installation {
        kind: InstallationKind::Standard,
        platform: Platform::MacOs,
        app_path,
        resource_path: resource_path.clone(),
        version,
        architecture: Some(probed_architecture),
        writable: is_probably_writable(&resource_path),
        confidence: if !require_existing && evidence.is_empty() {
            Confidence::Low
        } else if evidence.len() > 1 {
            Confidence::High
        } else {
            Confidence::Medium
        },
        evidence,
    })
}

fn discover_portable_macos(root: &Path) -> Option<Installation> {
    let app_path = fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().contains("reaper"))
        })?;

    let ini_path = root.join("reaper.ini");
    let mut evidence = vec![Evidence::new(
        "portable-macos-app-bundle",
        Some(app_path.clone()),
        "Found a REAPER app bundle in the selected portable folder.",
    )];
    let confidence = if ini_path.exists() {
        evidence.push(Evidence::new(
            "portable-macos-reaper-ini",
            Some(ini_path),
            "Found reaper.ini in the selected portable folder.",
        ));
        Confidence::High
    } else {
        Confidence::Medium
    };

    let version = file_version(&app_path).ok().flatten();
    if let Some(version) = &version {
        evidence.push(Evidence::new(
            "portable-macos-app-version",
            Some(app_path.clone()),
            format!("Read REAPER version {version} from app metadata."),
        ));
    }

    let probed_architecture = probe_executable_architecture(&app_path);
    Some(Installation {
        kind: InstallationKind::Portable,
        platform: Platform::MacOs,
        app_path,
        resource_path: root.to_path_buf(),
        version,
        architecture: Some(probed_architecture),
        writable: is_probably_writable(root),
        confidence,
        evidence,
    })
}

fn is_probably_writable(path: &Path) -> bool {
    let existing_path = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    fs::metadata(existing_path)
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false)
}

/// Detect a JAWS-for-REAPER scripts install by reading the version that the
/// vendor NSIS installer left on disk. The flow is:
///
///   1. Read the `Reaper_JawsScripts` Programs-and-Features uninstall key's
///      `UninstallDirectory` REG_SZ value (HKLM\SOFTWARE\WoW6432Node\… on
///      64-bit Windows). The NSIS installer writes this value during install.
///   2. Read the StringFileInfo "FileVersion" resource off
///      `<dir>\Uninstall.exe`. The script author bumps it per release, so
///      it's the most reliable on-disk version stamp for users who haven't
///      let RABBIT install the package yet (the receipt detector handles the
///      RABBIT-managed case earlier).
///
/// Returns `None` when the registry key is missing, the uninstaller is
/// missing, or the FileVersion resource cannot be parsed as a RABBIT `Version`.
/// Always `None` on non-Windows hosts.
fn detect_jaws_scripts_via_uninstall_exe(spec: &PackageSpec) -> Option<ComponentDetection> {
    let install_dir =
        rabbit_platform::read_uninstall_value("Reaper_JawsScripts", "UninstallDirectory")?;
    let uninstall_exe = PathBuf::from(install_dir).join("Uninstall.exe");
    if !uninstall_exe.is_file() {
        return None;
    }
    let raw = rabbit_platform::read_file_version_string(&uninstall_exe)?;
    let version = crate::version::Version::parse(&raw).ok()?;
    Some(ComponentDetection {
        package_id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        installed: true,
        version: Some(version),
        detector: "jaws-scripts-uninstall-exe".to_string(),
        confidence: Confidence::High,
        files: vec![uninstall_exe],
        notes: vec![
            "Version came from the JAWS-for-REAPER scripts vendor uninstaller's FileVersion resource."
                .to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        DiscoveryOptions, default_standard_installation, detect_components, detect_ffmpeg_version,
        discover_installations, embedded_snapshot_version_from_text,
        ffmpeg_major_from_libavformat_major, ffmpeg_version_from_product_version_string,
        libavformat_major_from_basename, reapack_version_from_text, sws_version_from_text,
    };
    use crate::model::Platform;
    use crate::package::{
        PACKAGE_FFMPEG, PACKAGE_OSARA, PACKAGE_REAKONTROL, PACKAGE_REAPACK, PACKAGE_SWS,
    };

    #[test]
    fn detects_extensions_by_user_plugin_prefix() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(plugins.join("reaper_osara64.dll"), b"").unwrap();
        fs::write(plugins.join("reaper_sws-x64.dll"), b"").unwrap();
        fs::write(plugins.join("reaper_reapack-x64.dll"), b"").unwrap();

        let detections = detect_components(dir.path(), Platform::Windows).unwrap();
        let installed: Vec<_> = detections
            .iter()
            .filter(|detection| detection.installed)
            .map(|detection| detection.package_id.as_str())
            .collect();

        assert!(installed.contains(&PACKAGE_OSARA));
        assert!(installed.contains(&PACKAGE_SWS));
        assert!(installed.contains(&PACKAGE_REAPACK));
    }

    #[test]
    fn ffmpeg_libavformat_filenames_round_trip_to_major_only_versions() {
        assert_eq!(libavformat_major_from_basename("avformat-62.dll"), Some(62));
        assert_eq!(
            libavformat_major_from_basename("AVFORMAT-62.DLL"),
            Some(62),
            "case-insensitive on Windows"
        );
        assert_eq!(
            libavformat_major_from_basename("libavformat.62.dylib"),
            Some(62),
            "macOS dylib layout"
        );
        assert_eq!(libavformat_major_from_basename("avformat.dll"), None);
        assert_eq!(libavformat_major_from_basename("avcodec-62.dll"), None);
        assert_eq!(libavformat_major_from_basename("avformat-foo.dll"), None);

        assert_eq!(ffmpeg_major_from_libavformat_major(58), Some(4));
        assert_eq!(ffmpeg_major_from_libavformat_major(62), Some(8));
        assert_eq!(ffmpeg_major_from_libavformat_major(53), None);
    }

    #[test]
    fn ffmpeg_product_version_parser_extracts_release_string() {
        // Plain stable: Gyan.dev's `ProductVersion`.
        assert_eq!(
            ffmpeg_version_from_product_version_string("8.1.1")
                .unwrap()
                .raw(),
            "8.1.1"
        );
        // BtbN/tordona-style with `n` prefix.
        assert_eq!(
            ffmpeg_version_from_product_version_string("n8.1.1")
                .unwrap()
                .raw(),
            "8.1.1"
        );
        // tordona-style autobuild trailer — we drop everything after
        // the version run.
        assert_eq!(
            ffmpeg_version_from_product_version_string("n8.1.1-6-gdeadbeef-tordona-ffmpeg")
                .unwrap()
                .raw(),
            "8.1.1"
        );
        // Older 4-component style.
        assert_eq!(
            ffmpeg_version_from_product_version_string("v8.1.1.0")
                .unwrap()
                .raw(),
            "8.1.1.0"
        );
        // Whitespace.
        assert_eq!(
            ffmpeg_version_from_product_version_string("  8.0  ")
                .unwrap()
                .raw(),
            "8.0"
        );
        // No version-shaped prefix.
        assert!(ffmpeg_version_from_product_version_string("git-master").is_none());
        assert!(ffmpeg_version_from_product_version_string("").is_none());
    }

    #[test]
    fn ffmpeg_version_falls_back_to_libavformat_filename_when_exe_has_no_versioninfo() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        let avformat = plugins.join("avformat-62.dll");
        let avcodec = plugins.join("avcodec-62.dll");
        // A stub ffmpeg.exe with no PE resource section — file_version
        // returns Ok(None) so the helper drops to the libavformat
        // fallback.
        let ffmpeg_exe = plugins.join("ffmpeg.exe");
        fs::write(&avformat, b"").unwrap();
        fs::write(&avcodec, b"").unwrap();
        fs::write(&ffmpeg_exe, b"").unwrap();

        let (version, detector, confidence, _notes) =
            detect_ffmpeg_version(&[avcodec.clone(), avformat.clone()]).unwrap();
        assert_eq!(version.raw(), "8.0.0");
        assert_eq!(detector, "ffmpeg-libavformat-major");
        assert_eq!(confidence, super::Confidence::Medium);

        // Without an avformat file in the list, neither probe applies
        // — we don't synthesize from avcodec / avutil because their
        // major-bump cadence isn't perfectly synchronized to FFmpeg's,
        // and the stubbed ffmpeg.exe has no readable VS_FIXEDFILEINFO.
        assert!(detect_ffmpeg_version(&[avcodec]).is_none());
    }

    #[test]
    fn parses_ffmpeg_version_from_show_banner_format_string() {
        // FFmpeg's `show_banner` literal is one contiguous string in
        // .rdata; embed the same shape in a fake "binary" and verify
        // the scanner extracts the version.
        let mut bytes = vec![0u8; 1024];
        bytes.extend_from_slice(
            b"\0\0\0%s version 8.1.1, Copyright (c) %d-%d the FFmpeg developers\n\0",
        );
        bytes.extend(std::iter::repeat(0u8).take(1024));
        let version = super::ffmpeg_version_from_binary_bytes(&bytes).unwrap();
        assert_eq!(version.raw(), "8.1.1");
    }

    #[test]
    fn parses_ffmpeg_snapshot_version_from_show_banner() {
        // tordona / BtbN snapshot builds substitute `n8.1.1-6-gdeadbeef`
        // (or similar) into FFMPEG_VERSION. The scanner strips the `n`
        // and keeps the leading digit-or-dot run.
        let mut bytes = vec![0u8; 256];
        bytes.extend_from_slice(
            b"%s version n8.1.1-6-gdeadbeef, Copyright (c) %d-%d the FFmpeg developers\n",
        );
        let version = super::ffmpeg_version_from_binary_bytes(&bytes).unwrap();
        assert_eq!(version.raw(), "8.1.1");
    }

    #[test]
    fn parses_ffmpeg_gyan_full_build_banner_without_comma() {
        // Gyan.dev's full builds patch the banner format to drop the
        // comma between the version and `Copyright` and pad the
        // version with trailing spaces. Real captured shape from a
        // 2026 Gyan release ffmpeg.exe (the `n` prefix is part of the
        // version suffix, not a snapshot marker):
        //   `%s version 8.1.1-full_build-www.gyan.dev         Copyright (c) %d-%d the FFmpeg developers`
        let mut bytes = vec![0u8; 256];
        bytes.extend_from_slice(
            b"%s version 8.1.1-full_build-www.gyan.dev         Copyright (c) %d-%d the FFmpeg developers \n",
        );
        let version = super::ffmpeg_version_from_binary_bytes(&bytes).unwrap();
        assert_eq!(version.raw(), "8.1.1");
    }

    #[test]
    fn binary_scan_returns_none_without_anchor() {
        // A non-FFmpeg binary won't have the distinctive
        // `the FFmpeg developers` anchor; falling through here lets the
        // caller drop to Probe 3 (libavformat filename heuristic).
        let bytes = b"random binary contents with version 9.9.9 inside but no anchor";
        assert!(super::ffmpeg_version_from_binary_bytes(bytes).is_none());
    }

    #[test]
    fn detects_externally_installed_ffmpeg_via_avformat_filename() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        // Drop an avformat-62.dll from a hypothetical external FFmpeg 8
        // install. RABBIT shouldn't have a receipt for it, so detection
        // must fall back to the libavformat-major heuristic.
        fs::write(plugins.join("avformat-62.dll"), b"").unwrap();
        fs::write(plugins.join("avcodec-62.dll"), b"").unwrap();

        let detections = detect_components(dir.path(), Platform::Windows).unwrap();
        let ffmpeg = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_FFMPEG)
            .expect("ffmpeg row missing from Windows detections");
        assert!(ffmpeg.installed);
        let version = ffmpeg.version.as_ref().expect("version not detected");
        assert_eq!(version.raw(), "8.0.0");
        assert_eq!(ffmpeg.detector, "ffmpeg-libavformat-major");
    }

    #[test]
    fn detects_windows_portable_installation_from_selected_folder() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("reaper.exe"), b"").unwrap();
        fs::write(dir.path().join("reaper.ini"), b"").unwrap();

        let installations = discover_installations(&DiscoveryOptions {
            include_standard: false,
            portable_roots: vec![dir.path().to_path_buf()],
        })
        .unwrap();

        if cfg!(target_os = "windows") {
            assert_eq!(installations.len(), 1);
        } else {
            assert!(installations.is_empty());
        }
    }

    #[test]
    fn parses_osara_snapshot_version_from_binary_text() {
        let version = embedded_snapshot_version_from_text("OSARA 2024.3.6.1332,13560ef7").unwrap();
        assert_eq!(version.raw(), "2024.3.6.1332");
    }

    #[test]
    fn parses_reakontrol_snapshot_version_from_binary_text() {
        let version =
            embedded_snapshot_version_from_text("reaKontrol 2026.2.16.100,abcdef0").unwrap();
        assert_eq!(version.raw(), "2026.2.16.100");
    }

    #[test]
    fn detects_reakontrol_version_by_binary_scan_when_metadata_is_unavailable() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(
            plugins.join("reaper_kontrol_mk2.dll"),
            b"reaKontrol\0snapshot\0 2026.2.16.100,abcdef0\0",
        )
        .unwrap();

        let detections =
            super::detect_components_with_probes(dir.path(), Platform::Windows, |_| None).unwrap();
        let reakontrol = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_REAKONTROL)
            .unwrap();

        assert_eq!(reakontrol.version.as_ref().unwrap().raw(), "2026.2.16.100");
        assert_eq!(reakontrol.detector, "reakontrol-binary-version-string");
    }

    #[test]
    fn detects_osara_version_by_binary_scan_when_metadata_is_unavailable() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(
            plugins.join("reaper_osara64.dll"),
            b"OSARA\0snapshot\0 2024.3.6.1332,13560ef7\0",
        )
        .unwrap();

        // Inject a no-op uninstall-registry probe so the test does not pick up
        // any OSARA install that happens to be present on the dev/CI host —
        // the binary-scan fallback is what we are exercising here.
        let detections =
            super::detect_components_with_probes(dir.path(), Platform::Windows, |_| None).unwrap();
        let osara = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_OSARA)
            .unwrap();

        assert_eq!(osara.version.as_ref().unwrap().raw(), "2024.3.6.1332");
        assert_eq!(osara.detector, "osara-binary-version-string");
    }

    #[test]
    fn parses_sws_version_with_commit_hash() {
        let version = sws_version_from_text("SWS Extension v2.14.0.1 #2dadf4b\0").unwrap();
        assert_eq!(version.raw(), "2.14.0.1");
    }

    #[test]
    fn parses_sws_three_component_version_with_commit_hash() {
        let version = sws_version_from_text("v2.14.0 #abcdef0\0").unwrap();
        assert_eq!(version.raw(), "2.14.0");
    }

    #[test]
    fn rejects_sws_version_pattern_without_commit_hash() {
        assert!(sws_version_from_text("plain 1.2.3 with no anchor").is_none());
    }

    #[test]
    fn detects_sws_version_by_binary_scan_when_metadata_is_unavailable() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(
            plugins.join("reaper_sws-x64.dll"),
            b"SWS Extension\0v2.14.0.1 #2dadf4b\0",
        )
        .unwrap();

        let detections =
            super::detect_components_with_probes(dir.path(), Platform::Windows, |_| None).unwrap();
        let sws = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_SWS)
            .unwrap();

        assert_eq!(sws.version.as_ref().unwrap().raw(), "2.14.0.1");
        assert_eq!(sws.detector, "sws-binary-version-string");
    }

    #[test]
    fn parses_reapack_version_from_user_agent() {
        let version =
            reapack_version_from_text("Mozilla/5.0 ReaPack/1.2.6 (Cockos REAPER)\0").unwrap();
        assert_eq!(version.raw(), "1.2.6");
    }

    #[test]
    fn parses_reapack_version_from_legacy_about_form() {
        let version = reapack_version_from_text("\0ReaPack v1.2.6\0").unwrap();
        assert_eq!(version.raw(), "1.2.6");
    }

    #[test]
    fn rejects_reapack_version_without_anchor() {
        assert!(reapack_version_from_text("just 1.2.6 by itself").is_none());
    }

    #[test]
    fn detects_reapack_version_by_binary_scan_when_metadata_is_unavailable() {
        let dir = tempdir().unwrap();
        let plugins = dir.path().join("UserPlugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(
            plugins.join("reaper_reapack-x64.dll"),
            b"User-Agent: ReaPack/1.2.6 (REAPER)\0",
        )
        .unwrap();

        let detections =
            super::detect_components_with_probes(dir.path(), Platform::Windows, |_| None).unwrap();
        let reapack = detections
            .iter()
            .find(|detection| detection.package_id == PACKAGE_REAPACK)
            .unwrap();

        assert_eq!(reapack.version.as_ref().unwrap().raw(), "1.2.6");
        assert_eq!(reapack.detector, "reapack-binary-version-string");
    }

    #[test]
    fn exposes_default_standard_installation_target() {
        let Some(platform) = Platform::current() else {
            return;
        };

        let installation = default_standard_installation(platform).unwrap();

        assert_eq!(installation.kind, crate::model::InstallationKind::Standard);
        assert_eq!(installation.platform, platform);
        match platform {
            Platform::Windows => {
                assert!(installation.resource_path.ends_with("REAPER"));
                assert!(installation.app_path.ends_with("reaper.exe"));
            }
            Platform::MacOs => {
                assert!(installation.resource_path.ends_with("REAPER"));
                assert!(installation.app_path.ends_with("REAPER.app"));
            }
        }
    }
}
