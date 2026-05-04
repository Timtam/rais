use std::path::Path;

use rabbit_platform::read_file_version_parts;

use crate::Result;
use crate::version::Version;

pub fn file_version(path: &Path) -> Result<Option<Version>> {
    let Some(parts) = read_file_version_parts(path) else {
        return Ok(None);
    };
    let version = version_string_for_path(path, &parts);
    Version::parse(version).map(Some)
}

fn version_string_for_path(path: &Path, parts: &[u32; 4]) -> String {
    if is_reaper_app_path(path) {
        if let Some(version) = reaper_version_string_from_parts(parts) {
            return version;
        }
    }

    trim_trailing_zero_parts(parts)
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn is_reaper_app_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("reaper.exe"))
}

fn reaper_version_string_from_parts(parts: &[u32; 4]) -> Option<String> {
    if parts[3] != 0 || parts[1] >= 10 || parts[2] >= 10 {
        return None;
    }

    Some(format!("{}.{}{}", parts[0], parts[1], parts[2]))
}

fn trim_trailing_zero_parts(parts: &[u32; 4]) -> &[u32] {
    let mut len = parts.len();
    while len > 2 && parts[len - 1] == 0 {
        len -= 1;
    }
    &parts[..len]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{trim_trailing_zero_parts, version_string_for_path};

    #[test]
    fn trims_trailing_zero_parts_but_keeps_major_minor() {
        assert_eq!(trim_trailing_zero_parts(&[7, 69, 0, 0]), &[7, 69]);
        assert_eq!(trim_trailing_zero_parts(&[2, 14, 0, 7]), &[2, 14, 0, 7]);
        assert_eq!(trim_trailing_zero_parts(&[1, 2, 6, 0]), &[1, 2, 6]);
    }

    #[test]
    fn normalizes_reaper_windows_fixed_file_versions() {
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 6, 9, 0]),
            "7.69"
        );
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 7, 0, 0]),
            "7.70"
        );
    }

    #[test]
    fn keeps_non_reaper_versions_in_standard_dotted_form() {
        assert_eq!(
            version_string_for_path(
                Path::new("/REAPER/UserPlugins/reaper_osara64.dll"),
                &[1, 2, 6, 0]
            ),
            "1.2.6"
        );
        assert_eq!(
            version_string_for_path(Path::new("/REAPER/reaper.exe"), &[7, 69, 0, 0]),
            "7.69"
        );
    }
}
