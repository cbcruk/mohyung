use anyhow::{Context, Result};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::types::{FileEntry, LinkEntry, PackageInfo};

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

fn find_bin_dirs(node_modules_path: &Path, use_pnpm: bool) -> Vec<PackageDir> {
    let mut dirs = Vec::new();

    let top_bin = node_modules_path.join(".bin");
    if top_bin.is_dir() {
        dirs.push(PackageDir {
            path: top_bin,
            relative_path: ".bin".to_string(),
        });
    }

    if use_pnpm {
        if let Ok(entries) = fs::read_dir(node_modules_path.join(".pnpm")) {
            for entry in entries.flatten() {
                let bin = entry.path().join("node_modules").join(".bin");
                if bin.is_dir() {
                    dirs.push(PackageDir {
                        path: bin,
                        relative_path: format!(
                            ".pnpm/{}/node_modules/.bin",
                            entry.file_name().to_string_lossy()
                        ),
                    });
                }
            }
        }
    }

    dirs
}

fn scan_bin_dir(pkg_dir: &PackageDir) -> Result<Option<ScannedPackage>> {
    let files = collect_files(&pkg_dir.path)?;
    if files.is_empty() {
        return Ok(None);
    }

    Ok(Some(ScannedPackage {
        info: PackageInfo {
            id: None,
            name: ".bin".to_string(),
            version: "0.0.0".to_string(),
            path: pkg_dir.relative_path.clone(),
        },
        files,
    }))
}

fn scan_package_files(pkg_dir: &PackageDir) -> Result<Option<ScannedPackage>> {
    let pkg_json_path = pkg_dir.path.join("package.json");
    let (name, version) = match parse_package_json(&pkg_json_path) {
        Some(parsed) => parsed,
        None => return Ok(None),
    };
    let files = collect_files(&pkg_dir.path)?;

    Ok(Some(ScannedPackage {
        info: PackageInfo {
            id: None,
            name,
            version,
            path: pkg_dir.relative_path.clone(),
        },
        files,
    }))
}

fn to_file_entry(entry: &walkdir::DirEntry, base: &Path) -> Result<FileEntry> {
    let metadata = entry
        .metadata()
        .with_context(|| format!("failed to read metadata: {}", entry.path().display()))?;
    let absolute_path = entry.path().to_path_buf();
    let relative_path = absolute_path
        .strip_prefix(base)?
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

    Ok(FileEntry {
        relative_path,
        absolute_path,
        mode,
        size: metadata.len(),
        mtime,
    })
}

fn collect_files(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut files = Vec::new();

    for entry in WalkDir::new(dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        files.push(to_file_entry(&entry, dir)?);
    }

    Ok(files)
}

fn scan_root_files(node_modules_path: &Path, use_pnpm: bool) -> Result<Option<ScannedPackage>> {
    let mut files = Vec::new();

    let mut roots = vec![node_modules_path.to_path_buf()];
    if use_pnpm {
        roots.push(node_modules_path.join(".pnpm"));
    }

    for root in roots {
        for entry in WalkDir::new(&root).min_depth(1).max_depth(1) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            files.push(to_file_entry(&entry, node_modules_path)?);
        }
    }

    if files.is_empty() {
        return Ok(None);
    }

    Ok(Some(ScannedPackage {
        info: PackageInfo {
            id: None,
            name: ".root".to_string(),
            version: "0.0.0".to_string(),
            path: ".".to_string(),
        },
        files,
    }))
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
    let bin_dirs = find_bin_dirs(node_modules_path, use_pnpm);

    if let Some(progress) = on_progress {
        progress(0, package_dirs.len(), "Collecting packages...");
    }

    let scanned: Result<Vec<Option<ScannedPackage>>> = package_dirs
        .par_iter()
        .map(scan_package_files)
        .collect();
    let mut packages: Vec<ScannedPackage> = scanned?.into_iter().flatten().collect();

    for bin_dir in &bin_dirs {
        if let Some(pkg) = scan_bin_dir(bin_dir)? {
            packages.push(pkg);
        }
    }
    if let Some(pkg) = scan_root_files(node_modules_path, use_pnpm)? {
        packages.push(pkg);
    }

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

pub fn scan_symlinks(node_modules_path: &Path) -> Result<Vec<LinkEntry>> {
    let mut links = Vec::new();

    for entry in WalkDir::new(node_modules_path).min_depth(1) {
        let entry = entry?;
        if !entry.path_is_symlink() {
            continue;
        }

        let target = fs::read_link(entry.path())?;
        let path = entry
            .path()
            .strip_prefix(node_modules_path)?
            .to_string_lossy()
            .to_string();

        links.push(LinkEntry {
            path,
            target: target.to_string_lossy().to_string(),
        });
    }

    Ok(links)
}

pub fn scan_empty_dirs(node_modules_path: &Path) -> Result<Vec<String>> {
    let mut dirs = Vec::new();

    for entry in WalkDir::new(node_modules_path).min_depth(1) {
        let entry = entry?;
        if !entry.file_type().is_dir() {
            continue;
        }
        if fs::read_dir(entry.path())?.next().is_some() {
            continue;
        }

        let path = entry
            .path()
            .strip_prefix(node_modules_path)?
            .to_string_lossy()
            .to_string();
        dirs.push(path);
    }

    Ok(dirs)
}

pub fn count_files(node_modules_path: &Path) -> Result<usize> {
    let count = WalkDir::new(node_modules_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count();
    Ok(count)
}
