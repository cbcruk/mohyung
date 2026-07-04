use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::core::store::Store;
use crate::utils::compression::decompress;
use crate::utils::fs::is_safe_relative_path;
use crate::utils::progress::truncate_message;

struct ExtractedFile {
    full_path: String,
    content: Arc<Vec<u8>>,
    mode: u32,
    mtime: i64,
}

pub fn extract_files(
    store: &Store,
    output_path: &Path,
    on_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(usize, u64)> {
    let files = store.get_all_files()?;
    let total_files = files.len();

    let mut blob_cache: HashMap<String, Vec<u8>> = HashMap::new();

    let mut total_size: u64 = 0;

    for (index, file) in files.iter().enumerate() {
        if let Some(progress) = on_progress {
            progress(
                index + 1,
                total_files,
                truncate_message(&file.record.relative_path, 40),
            );
        }

        let full_path = Path::new(output_path)
            .join(&file.package_path)
            .join(&file.record.relative_path);

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = if let Some(cached) = blob_cache.get(&file.record.blob_hash) {
            cached.clone()
        } else {
            let compressed = store.get_blob(&file.record.blob_hash)?.ok_or_else(|| {
                anyhow!(
                    "blob {} missing for {}",
                    file.record.blob_hash,
                    file.record.relative_path
                )
            })?;
            let decompressed = decompress(&compressed)?;

            if decompressed.len() < 100 * 1024 {
                blob_cache.insert(file.record.blob_hash.clone(), decompressed.clone());
            }

            decompressed
        };

        total_size += content.len() as u64;
        fs::write(&full_path, &content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file.record.mode != 0 {
                let _ = fs::set_permissions(
                    &full_path,
                    fs::Permissions::from_mode(file.record.mode & 0o777),
                );
            }
        }
    }

    Ok((total_files, total_size))
}

pub fn restore_empty_dirs(store: &Store, output_path: &Path) -> Result<usize> {
    let dirs = store.get_all_dirs()?;

    for dir in &dirs {
        if !is_safe_relative_path(dir) {
            bail!("unsafe dir path in database: {}", dir);
        }
        fs::create_dir_all(output_path.join(dir))?;
    }

    Ok(dirs.len())
}

pub fn restore_links(store: &Store, output_path: &Path) -> Result<usize> {
    let links = store.get_all_links()?;

    for link in &links {
        if !is_safe_relative_path(&link.path) {
            bail!("unsafe link path in database: {}", link.path);
        }

        let link_path = output_path.join(&link.path);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)?;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&link.target, &link_path)?;

        #[cfg(windows)]
        {
            let resolved = link_path
                .parent()
                .map(|p| p.join(&link.target))
                .unwrap_or_else(|| Path::new(&link.target).to_path_buf());
            let result = if resolved.is_dir() {
                std::os::windows::fs::symlink_dir(&link.target, &link_path)
            } else {
                std::os::windows::fs::symlink_file(&link.target, &link_path)
            };
            if let Err(e) = result {
                eprintln!(
                    "Warning: failed to create symlink {}: {}",
                    link_path.display(),
                    e
                );
            }
        }
    }

    Ok(links.len())
}

const BATCH_SIZE: usize = 512;

pub fn extract_files_parallel(
    store: &Store,
    output_path: &Path,
    on_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(usize, u64)> {
    let files = store.get_all_files()?;
    let total_files = files.len();

    let mut total_size: u64 = 0;
    let mut written: usize = 0;
    let mut last_blob: Option<(String, Arc<Vec<u8>>)> = None;

    for chunk in files.chunks(BATCH_SIZE) {
        let mut prepared: Vec<ExtractedFile> = Vec::with_capacity(chunk.len());

        for file in chunk {
            if !is_safe_relative_path(&file.package_path)
                || !is_safe_relative_path(&file.record.relative_path)
            {
                bail!(
                    "unsafe path in database: {}/{}",
                    file.package_path,
                    file.record.relative_path
                );
            }

            let content = match &last_blob {
                Some((hash, data)) if *hash == file.record.blob_hash => Arc::clone(data),
                _ => {
                    let compressed = store.get_blob(&file.record.blob_hash)?.ok_or_else(|| {
                        anyhow!(
                            "blob {} missing for {}",
                            file.record.blob_hash,
                            file.record.relative_path
                        )
                    })?;
                    let decompressed = Arc::new(decompress(&compressed)?);
                    last_blob = Some((file.record.blob_hash.clone(), Arc::clone(&decompressed)));
                    decompressed
                }
            };

            let full_path = Path::new(output_path)
                .join(&file.package_path)
                .join(&file.record.relative_path)
                .to_string_lossy()
                .to_string();

            prepared.push(ExtractedFile {
                full_path,
                content,
                mode: file.record.mode,
                mtime: file.record.mtime,
            });
        }

        total_size += prepared
            .par_iter()
            .map(|ef| -> Result<u64> {
                let path = Path::new(&ef.full_path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(path, ef.content.as_slice())
                    .with_context(|| format!("failed to write {}", path.display()))?;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if ef.mode != 0 {
                        fs::set_permissions(path, fs::Permissions::from_mode(ef.mode & 0o777))
                            .with_context(|| {
                                format!("failed to set permissions on {}", path.display())
                            })?;
                    }
                }

                if ef.mtime > 0 {
                    let mtime = std::time::UNIX_EPOCH
                        + std::time::Duration::from_millis(ef.mtime as u64);
                    fs::File::options()
                        .write(true)
                        .open(path)
                        .and_then(|f| f.set_modified(mtime))
                        .with_context(|| format!("failed to set mtime on {}", path.display()))?;
                }

                Ok(ef.content.len() as u64)
            })
            .try_reduce(|| 0, |a, b| Ok(a + b))?;

        written += chunk.len();
        if let Some(progress) = on_progress {
            progress(written, total_files, "Writing files...");
        }
    }

    Ok((total_files, total_size))
}
