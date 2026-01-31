use anyhow::{bail, Result};
use rayon::prelude::*;
use rusqlite::params;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::hasher::{hash_buffer, hash_string};
use crate::core::scanner::scan_node_modules;
use crate::core::store::Store;
use crate::types::PackOptions;
use crate::utils::compression::compress;
use crate::utils::fs::format_bytes;
use crate::utils::progress::create_progress_bar;

struct ProcessedFile {
    package_index: usize,
    hash: String,
    compressed: Option<Vec<u8>>,
    original_size: u64,
    mode: u32,
    mtime: i64,
    relative_path: String,
}

pub fn pack(options: &PackOptions) -> Result<()> {
    let node_modules_path = Path::new(&options.source).canonicalize()?;
    let db_path = fs::canonicalize(Path::new(&options.output).parent().unwrap_or(Path::new(".")))
        .unwrap_or_default()
        .join(
            Path::new(&options.output)
                .file_name()
                .unwrap_or_default(),
        );

    if !node_modules_path.exists() {
        bail!("node_modules not found: {}", node_modules_path.display());
    }

    eprintln!("Scanning {}...", node_modules_path.display());

    let scan_pb = create_progress_bar(100);
    let scan_result = scan_node_modules(&node_modules_path, Some(&|current, total, msg| {
        scan_pb.set_length(total as u64);
        scan_pb.set_position(current as u64);
        scan_pb.set_message(msg.to_string());
    }))?;
    scan_pb.finish_and_clear();

    eprintln!(
        "Found {} packages, {} files ({})",
        scan_result.packages.len(),
        scan_result.total_files,
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

    let mut store = Store::open(db_path.to_str().unwrap_or_default())?;

    store.set_metadata("created_at", &chrono_now())?;
    store.set_metadata("source_path", &node_modules_path.to_string_lossy())?;

    if options.include_lockfile {
        let lockfile_path = node_modules_path.join("..").join("package-lock.json");
        if lockfile_path.exists() {
            let content = fs::read_to_string(&lockfile_path)?;
            store.set_metadata("lockfile_hash", &hash_string(&content))?;
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

    let processed: Vec<ProcessedFile> = all_files
        .par_iter()
        .filter_map(|(pi, _fi, file)| {
            let content = fs::read(&file.absolute_path).ok()?;
            let hash = hash_buffer(&content);
            let compressed = compress(&content, compression_level);

            let count = processed_count.fetch_add(1, Ordering::Relaxed) + 1;
            let display = if file.relative_path.len() > 40 {
                &file.relative_path[..40]
            } else {
                &file.relative_path
            };
            pack_pb.set_position(count as u64);
            pack_pb.set_message(display.to_string());

            Some(ProcessedFile {
                package_index: *pi,
                hash,
                compressed: Some(compressed),
                original_size: content.len() as u64,
                mode: file.mode,
                mtime: file.mtime,
                relative_path: file.relative_path.clone(),
            })
        })
        .collect();

    pack_pb.finish_and_clear();

    eprintln!("Writing to database...");

    let mut deduplicated_count: usize = 0;
    let mut seen_hashes = std::collections::HashSet::new();

    store.transaction(|tx| {
        let mut insert_pkg_stmt = tx.prepare_cached(
            "INSERT INTO packages (name, version, path) VALUES (?1, ?2, ?3)
             ON CONFLICT(name, version, path) DO UPDATE SET name = name
             RETURNING id",
        )?;
        let mut insert_blob_stmt = tx.prepare_cached(
            "INSERT OR IGNORE INTO blobs (hash, content, original_size, compressed_size)
             VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut insert_file_stmt = tx.prepare_cached(
            "INSERT INTO files (package_id, relative_path, blob_hash, mode, mtime)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(package_id, relative_path) DO UPDATE SET
               blob_hash = excluded.blob_hash,
               mode = excluded.mode,
               mtime = excluded.mtime",
        )?;

        let mut package_ids: Vec<Option<i64>> = vec![None; scan_result.packages.len()];

        for pf in &processed {
            let pkg_id = if let Some(id) = package_ids[pf.package_index] {
                id
            } else {
                let pkg = &scan_result.packages[pf.package_index];
                let id: i64 = insert_pkg_stmt.query_row(
                    params![pkg.info.name, pkg.info.version, pkg.info.path],
                    |row| row.get(0),
                )?;
                package_ids[pf.package_index] = Some(id);
                id
            };

            if !seen_hashes.contains(&pf.hash) {
                if let Some(ref compressed) = pf.compressed {
                    insert_blob_stmt.execute(params![
                        pf.hash,
                        compressed,
                        pf.original_size,
                        compressed.len() as u64
                    ])?;
                    seen_hashes.insert(pf.hash.clone());
                }
            } else {
                deduplicated_count += 1;
            }

            insert_file_stmt.execute(params![
                pkg_id,
                pf.relative_path,
                pf.hash,
                pf.mode,
                pf.mtime
            ])?;
        }

        Ok(())
    })?;

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
        ],
        "\x1b[32m",
    );

    Ok(())
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let remaining = now % secs_per_day;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;

    let mut year = 1970i32;
    let mut remaining_days = days as i32;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i32; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &days) in days_in_months.iter().enumerate() {
        if remaining_days < days {
            month = i + 1;
            break;
        }
        remaining_days -= days;
    }

    let day = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
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
    eprintln!(
        "{}└{}┘{}",
        color,
        "─".repeat(width),
        reset
    );
}
