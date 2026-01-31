use anyhow::Result;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::types::{FileEntry, PackageInfo};

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub packages: Vec<ScannedPackage>,
    pub total_files: usize,
    pub total_size: u64,
}

#[derive(Debug, Clone)]
pub struct ScannedPackage {
    pub info: PackageInfo,
    pub files: Vec<FileEntry>,
}

#[derive(Debug)]
struct PackageDir {
    path: PathBuf,
    relative_path: String,
}

fn is_pnpm_structure(node_modules_path: &Path) -> bool {
    node_modules_path.join(".pnpm").exists()
}

fn parse_package_json(pkg_json_path: &Path) -> Option<(String, String)> {
    let content = fs::read_to_string(pkg_json_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;

    let name = parsed["name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let version = parsed["version"]
        .as_str()
        .unwrap_or("0.0.0")
        .to_string();

    Some((name, version))
}

fn find_package_dirs(node_modules_path: &Path) -> Result<Vec<PackageDir>> {
    let mut dirs = Vec::new();
    let entries = fs::read_dir(node_modules_path)?;

    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".bin" || name == ".cache" || name == ".pnpm" {
            continue;
        }

        let full_path = entry.path();

        if name.starts_with('@') {
            let scoped_entries = fs::read_dir(&full_path)?;
            for scoped_entry in scoped_entries {
                let scoped_entry = scoped_entry?;
                if !scoped_entry.file_type()?.is_dir() {
                    continue;
                }
                let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                dirs.push(PackageDir {
                    path: scoped_entry.path(),
                    relative_path: format!("{}/{}", name, scoped_name),
                });
            }
        } else {
            dirs.push(PackageDir {
                path: full_path,
                relative_path: name,
            });
        }
    }

    Ok(dirs)
}

fn find_pnpm_package_dirs(node_modules_path: &Path) -> Result<Vec<PackageDir>> {
    let mut dirs = Vec::new();
    let pnpm_path = node_modules_path.join(".pnpm");
    let entries = fs::read_dir(&pnpm_path)?;

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name == "node_modules" || name.starts_with('.') {
            continue;
        }

        let inner_node_modules = entry.path().join("node_modules");
        if !inner_node_modules.exists() {
            continue;
        }

        let inner_entries = fs::read_dir(&inner_node_modules)?;
        for inner_entry in inner_entries {
            let inner_entry = inner_entry?;
            if !inner_entry.file_type()?.is_dir() {
                continue;
            }

            let inner_name = inner_entry.file_name().to_string_lossy().to_string();
            if inner_name == ".bin" {
                continue;
            }

            let pkg_path = inner_entry.path();

            if inner_name.starts_with('@') {
                let scoped_entries = fs::read_dir(&pkg_path)?;
                for scoped_entry in scoped_entries {
                    let scoped_entry = scoped_entry?;
                    if !scoped_entry.file_type()?.is_dir() {
                        continue;
                    }
                    let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                    dirs.push(PackageDir {
                        path: scoped_entry.path(),
                        relative_path: format!(
                            ".pnpm/{}/node_modules/{}/{}",
                            name, inner_name, scoped_name
                        ),
                    });
                }
            } else {
                dirs.push(PackageDir {
                    path: pkg_path,
                    relative_path: format!(".pnpm/{}/node_modules/{}", name, inner_name),
                });
            }
        }
    }

    Ok(dirs)
}

fn scan_package_files(pkg_dir: &PackageDir) -> Option<ScannedPackage> {
    let pkg_json_path = pkg_dir.path.join("package.json");
    let (name, version) = parse_package_json(&pkg_json_path)?;

    let mut files = Vec::new();

    for entry in WalkDir::new(&pkg_dir.path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let metadata = entry.metadata().ok()?;
        let absolute_path = entry.path().to_path_buf();
        let relative_path = absolute_path
            .strip_prefix(&pkg_dir.path)
            .ok()?
            .to_string_lossy()
            .to_string();

        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = 0o644u32;

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        files.push(FileEntry {
            relative_path,
            absolute_path,
            mode,
            size: metadata.len(),
            mtime,
        });
    }

    Some(ScannedPackage {
        info: PackageInfo {
            id: None,
            name,
            version,
            path: pkg_dir.relative_path.clone(),
        },
        files,
    })
}

pub fn scan_node_modules(
    node_modules_path: &Path,
    on_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<ScanResult> {
    let use_pnpm = is_pnpm_structure(node_modules_path);

    let package_dirs = if use_pnpm {
        find_pnpm_package_dirs(node_modules_path)?
    } else {
        find_package_dirs(node_modules_path)?
    };

    if let Some(progress) = on_progress {
        progress(0, package_dirs.len(), "Collecting packages...");
    }

    let packages: Vec<ScannedPackage> = package_dirs
        .par_iter()
        .filter_map(|pkg_dir| scan_package_files(pkg_dir))
        .collect();

    let total_files: usize = packages.iter().map(|p| p.files.len()).sum();
    let total_size: u64 = packages
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|f| f.size)
        .sum();

    if let Some(progress) = on_progress {
        progress(package_dirs.len(), package_dirs.len(), "Done");
    }

    Ok(ScanResult {
        packages,
        total_files,
        total_size,
    })
}

pub fn count_files(node_modules_path: &Path) -> Result<usize> {
    let count = WalkDir::new(node_modules_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count();
    Ok(count)
}
