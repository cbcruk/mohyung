use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::core::hasher::to_hex;
use crate::core::store::Store;
use crate::types::ProgressFn;
use crate::utils::compression::decompress;
use crate::utils::fs::is_safe_relative_path;

struct ExtractedFile {
    full_path: String,
    content: Arc<Vec<u8>>,
    // Only consumed by the `#[cfg(unix)]` permission-setting path below.
    #[cfg_attr(not(unix), allow(dead_code))]
    mode: u32,
    mtime: i64,
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
    on_progress: Option<ProgressFn>,
) -> Result<(usize, u64)> {
    let files = store.get_all_files()?;
    let total_files = files.len();

    let mut total_size: u64 = 0;
    let mut written: usize = 0;

    for chunk in files.chunks(BATCH_SIZE) {
        // Validate every path before touching blob data, so a malicious DB is
        // rejected up front rather than failing later during decompression.
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
        }

        // Fetch each distinct compressed blob once (DB access must stay serial:
        // rusqlite Connection is not Sync). Files are ordered by blob_hash, so a
        // blob's files are contiguous and the distinct set per chunk is small.
        let mut order: Vec<&[u8]> = Vec::new();
        let mut seen: HashSet<&[u8]> = HashSet::new();
        for file in chunk {
            if seen.insert(&file.record.blob_hash) {
                order.push(&file.record.blob_hash);
            }
        }

        let compressed: Vec<(&[u8], Vec<u8>)> = order
            .into_iter()
            .map(|hash| {
                let data = store
                    .get_blob(hash)?
                    .ok_or_else(|| anyhow!("blob {} missing in database", to_hex(hash)))?;
                Ok((hash, data))
            })
            .collect::<Result<_>>()?;

        // Decompression is CPU-bound; run it in parallel across distinct blobs.
        let blobs: HashMap<&[u8], Arc<Vec<u8>>> = compressed
            .par_iter()
            .map(|(hash, data)| Ok((*hash, Arc::new(decompress(data)?))))
            .collect::<Result<_>>()?;

        let mut prepared: Vec<ExtractedFile> = Vec::with_capacity(chunk.len());
        for file in chunk {
            let content = blobs
                .get(file.record.blob_hash.as_slice())
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "blob {} missing for {}",
                        to_hex(&file.record.blob_hash),
                        file.record.relative_path
                    )
                })?;

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
                    let mtime =
                        std::time::UNIX_EPOCH + std::time::Duration::from_millis(ef.mtime as u64);
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
