use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::core::store::Store;
use crate::utils::compression::decompress;

struct ExtractedFile {
    full_path: String,
    content: Vec<u8>,
    mode: u32,
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
            let display = &file.record.relative_path;
            let display = if display.len() > 40 {
                &display[..40]
            } else {
                display
            };
            progress(index + 1, total_files, display);
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
            let compressed = match store.get_blob(&file.record.blob_hash)? {
                Some(data) => data,
                None => {
                    eprintln!("Blob not found: {}", file.record.relative_path);
                    continue;
                }
            };
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

pub fn extract_files_parallel(
    store: &Store,
    output_path: &Path,
    on_progress: Option<&dyn Fn(usize, usize, &str)>,
) -> Result<(usize, u64)> {
    let files = store.get_all_files()?;
    let total_files = files.len();

    if let Some(progress) = on_progress {
        progress(0, total_files, "Reading blobs...");
    }

    let mut prepared: Vec<ExtractedFile> = Vec::with_capacity(total_files);
    let mut blob_cache: HashMap<String, Vec<u8>> = HashMap::new();

    for file in &files {
        let content = if let Some(cached) = blob_cache.get(&file.record.blob_hash) {
            cached.clone()
        } else {
            let compressed = match store.get_blob(&file.record.blob_hash)? {
                Some(data) => data,
                None => {
                    eprintln!("Blob not found: {}", file.record.relative_path);
                    continue;
                }
            };
            let decompressed = decompress(&compressed)?;

            if decompressed.len() < 100 * 1024 {
                blob_cache.insert(file.record.blob_hash.clone(), decompressed.clone());
            }

            decompressed
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
        });
    }

    drop(blob_cache);

    if let Some(progress) = on_progress {
        progress(total_files / 2, total_files, "Writing files...");
    }

    let total_size: u64 = prepared
        .par_iter()
        .map(|ef| {
            let path = Path::new(&ef.full_path);
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, &ef.content);

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if ef.mode != 0 {
                    let _ = fs::set_permissions(
                        path,
                        fs::Permissions::from_mode(ef.mode & 0o777),
                    );
                }
            }

            ef.content.len() as u64
        })
        .sum();

    if let Some(progress) = on_progress {
        progress(total_files, total_files, "Done");
    }

    Ok((total_files, total_size))
}
