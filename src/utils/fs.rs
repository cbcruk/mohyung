use std::path::{Component, Path};

pub fn is_safe_relative_path(path: &str) -> bool {
    let p = Path::new(path);
    if p.is_absolute() {
        return false;
    }
    !p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    })
}

/// Normalize OS path separators to `/` so stored paths are portable across
/// platforms (a DB packed on Windows must restore correctly on Unix, and vice
/// versa). No-op where the platform separator is already `/`, which also leaves
/// legitimate backslashes in Unix filenames untouched.
pub fn normalize_separators(path: &str) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        path.to_string()
    } else {
        path.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];

    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_relative_path() {
        assert!(is_safe_relative_path("foo/bar.js"));
        assert!(is_safe_relative_path(".bin/foo"));
        assert!(!is_safe_relative_path("../escape"));
        assert!(!is_safe_relative_path("foo/../../escape"));
        assert!(!is_safe_relative_path("/etc/passwd"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0.0 B");
        assert_eq!(format_bytes(512), "512.0 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }
}
