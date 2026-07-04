use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::core::hasher::{hash_buffer, hash_string};
use crate::core::scanner::{scan_empty_dirs, scan_node_modules, scan_symlinks};
use crate::core::store::{self, Store};
use crate::types::PackOptions;
use crate::utils::compression::compress;
use crate::utils::fs::format_bytes;
use crate::utils::progress::{create_progress_bar, truncate_message};

const BATCH_SIZE: usize = 512;

pub const LOCKFILE_NAMES: [&str; 3] = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock"];

struct ProcessedFile {
    package_index: usize,
    hash: String,
    compressed: Vec<u8>,
    original_size: u64,
    mode: u32,
    mtime: i64,
    relative_path: String,
}

pub fn pack(options: &PackOptions) -> Result<()> {
    let node_modules_path = Path::new(&options.source).canonicalize()?;
    let db_path = fs::canonicalize(
        Path::new(&options.output)
            .parent()
            .unwrap_or(Path::new(".")),
    )
    .unwrap_or_default()
    .join(Path::new(&options.output).file_name().unwrap_or_default());

    if !node_modules_path.exists() {
        bail!("node_modules not found: {}", node_modules_path.display());
    }

    eprintln!("Scanning {}...", node_modules_path.display());

    let scan_pb = create_progress_bar(100);
    let scan_result = scan_node_modules(
        &node_modules_path,
        Some(&|current, total, msg| {
            scan_pb.set_length(total as u64);
            scan_pb.set_position(current as u64);
            scan_pb.set_message(msg.to_string());
        }),
    )?;
    scan_pb.finish_and_clear();

    let links = scan_symlinks(&node_modules_path)?;
    let empty_dirs = scan_empty_dirs(&node_modules_path)?;

    eprintln!(
        "Found {} packages, {} files, {} symlinks ({})",
        scan_result.packages.len(),
        scan_result.total_files,
        links.len(),
        format_bytes(scan_result.total_size),
    );

    if db_path.exists() {
        fs::remove_file(&db_path)?;
        let wal = db_path.with_extension("db-wal");
        let shm = db_path.with_extension("db-shm");
        if wal.exists() {
            fs::remove_file(&wal)?;
        }
        if shm.exists() {
            fs::remove_file(&shm)?;
        }
    }

    let mut store = Store::create(db_path.to_str().unwrap_or_default())?;

    store.set_metadata("created_at", &now_rfc3339())?;
    store.set_metadata("source_path", &node_modules_path.to_string_lossy())?;

    if options.include_lockfile {
        if let Some(project_dir) = node_modules_path.parent() {
            for name in LOCKFILE_NAMES {
                let lockfile_path = project_dir.join(name);
                if lockfile_path.exists() {
                    let content = fs::read_to_string(&lockfile_path)?;
                    store.set_metadata("lockfile_name", name)?;
                    store.set_metadata("lockfile_hash", &hash_string(&content))?;
                    break;
                }
            }
        }
    }

    eprintln!("Packing files...");

    let pack_pb = create_progress_bar(scan_result.total_files as u64);
    let processed_count = AtomicUsize::new(0);
    let compression_level = options.compression_level;

    let all_files: Vec<(usize, usize, &crate::types::FileEntry)> = scan_result
        .packages
        .iter()
        .enumerate()
        .flat_map(|(pi, pkg)| {
            pkg.files
                .iter()
                .enumerate()
                .map(move |(fi, file)| (pi, fi, file))
        })
        .collect();

    let mut deduplicated_count: usize = 0;
    let mut seen_hashes = std::collections::HashSet::new();

    store.transaction(|tx| {
        for link in &links {
            store::insert_link(tx, link)?;
        }
        for dir in &empty_dirs {
            store::insert_dir(tx, dir)?;
        }

        let mut package_ids: Vec<Option<i64>> = vec![None; scan_result.packages.len()];

        for chunk in all_files.chunks(BATCH_SIZE) {
            let processed: Result<Vec<ProcessedFile>> = chunk
                .par_iter()
                .map(|(pi, _fi, file)| {
                    let content = fs::read(&file.absolute_path).with_context(|| {
                        format!("failed to read {}", file.absolute_path.display())
                    })?;
                    let hash = hash_buffer(&content);
                    let compressed = compress(&content, compression_level);

                    let count = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
                    pack_pb.set_position(count as u64);
                    pack_pb.set_message(truncate_message(&file.relative_path, 40).to_string());

                    Ok(ProcessedFile {
                        package_index: *pi,
                        hash,
                        compressed,
                        original_size: content.len() as u64,
                        mode: file.mode,
                        mtime: file.mtime,
                        relative_path: file.relative_path.clone(),
                    })
                })
                .collect();

            for pf in processed? {
                let pkg_id = if let Some(id) = package_ids[pf.package_index] {
                    id
                } else {
                    let pkg = &scan_result.packages[pf.package_index];
                    let id = store::insert_package(tx, &pkg.info)?;
                    package_ids[pf.package_index] = Some(id);
                    id
                };

                if seen_hashes.insert(pf.hash.clone()) {
                    store::insert_blob(tx, &pf.hash, &pf.compressed, pf.original_size)?;
                } else {
                    deduplicated_count += 1;
                }

                store::insert_file(tx, pkg_id, &pf.relative_path, &pf.hash, pf.mode, pf.mtime)?;
            }
        }

        Ok(())
    })?;

    pack_pb.finish_and_clear();

    store.finalize()?;

    let db_size = fs::metadata(&db_path)?.len();
    let compression_ratio = if scan_result.total_size > 0 {
        (1.0 - db_size as f64 / scan_result.total_size as f64) * 100.0
    } else {
        0.0
    };

    print_box(
        "Pack Complete",
        &[
            &format!("Output: {}", db_path.display()),
            &format!("Original: {}", format_bytes(scan_result.total_size)),
            &format!("DB size: {}", format_bytes(db_size)),
            &format!("Compression: {:.1}%", compression_ratio),
            &format!("Deduplicated: {}", deduplicated_count),
            &format!("Symlinks: {}", links.len()),
        ],
        "\x1b[32m",
    );

    Ok(())
}

fn now_rfc3339() -> String {
    let now = OffsetDateTime::now_utc();
    now.replace_nanosecond(0)
        .unwrap_or(now)
        .format(&Rfc3339)
        .unwrap_or_default()
}

pub fn print_box(title: &str, lines: &[&str], color: &str) {
    let reset = "\x1b[0m";
    let max_width = lines
        .iter()
        .map(|l| l.len())
        .max()
        .unwrap_or(0)
        .max(title.len() + 4);
    let width = max_width + 2;

    eprintln!(
        "{}┌─ {} {}─┐{}",
        color,
        title,
        "─".repeat(width.saturating_sub(title.len() + 4)),
        reset
    );
    for line in lines {
        eprintln!(
            "{}│{} {}{:<pad$} {}│{}",
            color,
            reset,
            line,
            "",
            color,
            reset,
            pad = width.saturating_sub(line.len() + 1)
        );
    }
    eprintln!("{}└{}┘{}", color, "─".repeat(width), reset);
}
