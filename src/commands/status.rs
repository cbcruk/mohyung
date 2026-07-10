use anyhow::{bail, Result};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use walkdir::WalkDir;

use crate::commands::pack::print_box;
use crate::core::hasher::{hash_buffer, hash_string};
use crate::core::scanner::scan_symlinks;
use crate::core::store::Store;
use crate::types::StatusResult;
use crate::utils::progress::{create_progress_bar, truncate_message};

fn join_rel(package_path: &str, relative_path: &str) -> String {
    if package_path == "." {
        relative_path.to_string()
    } else {
        format!("{}/{}", package_path, relative_path)
    }
}

pub fn status(db: &str, node_modules: &str, verbose: bool) -> Result<StatusResult> {
    let db_path = Path::new(db);
    let node_modules_path = Path::new(node_modules);

    if !db_path.exists() {
        bail!("Database not found: {}", db_path.display());
    }

    if !node_modules_path.exists() {
        eprintln!("node_modules not found: {}", node_modules_path.display());
        eprintln!("Run \"mohyung unpack\" to restore from database.");
        return Ok(StatusResult::default());
    }

    eprintln!("Comparing...");
    eprintln!("DB: {}", db_path.display());
    eprintln!("node_modules: {}", node_modules_path.display());

    let store = Store::open_readonly(db_path.to_str().unwrap_or_default())?;
    let files = store.get_all_files()?;
    let total = files.len();

    let pb = create_progress_bar(total as u64);
    let result = Mutex::new(StatusResult::default());

    files.par_iter().enumerate().for_each(|(index, file)| {
        let relative_path = join_rel(&file.package_path, &file.record.relative_path);
        let full_path = node_modules_path.join(&relative_path);

        pb.set_position((index + 1) as u64);
        pb.set_message(truncate_message(&file.record.relative_path, 40).to_string());

        if !full_path.exists() {
            result.lock().unwrap().only_in_db.push(relative_path);
            return;
        }

        match std::fs::read(&full_path) {
            Ok(content) => {
                let fs_hash = hash_buffer(&content);
                if fs_hash.as_slice() != file.record.blob_hash.as_slice() {
                    result.lock().unwrap().modified.push(relative_path);
                } else {
                    result.lock().unwrap().unchanged += 1;
                }
            }
            Err(_) => {
                result.lock().unwrap().modified.push(relative_path);
            }
        }
    });

    pb.finish_and_clear();

    let mut result = result.into_inner().unwrap();

    let db_paths: HashSet<String> = files
        .iter()
        .map(|f| join_rel(&f.package_path, &f.record.relative_path))
        .collect();

    for entry in WalkDir::new(node_modules_path) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(node_modules_path)?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with(".cache/") {
            continue;
        }
        if !db_paths.contains(&rel) {
            result.only_in_fs.push(rel);
        }
    }

    let db_links: HashMap<String, String> = store
        .get_all_links()?
        .into_iter()
        .map(|l| (l.path, l.target))
        .collect();
    let fs_links = scan_symlinks(node_modules_path)?;
    let fs_link_paths: HashSet<&str> = fs_links.iter().map(|l| l.path.as_str()).collect();

    for link in &fs_links {
        match db_links.get(&link.path) {
            Some(target) if *target == link.target => result.unchanged += 1,
            Some(_) => result.modified.push(link.path.clone()),
            None => result.only_in_fs.push(link.path.clone()),
        }
    }
    for path in db_links.keys() {
        if !fs_link_paths.contains(path.as_str()) {
            result.only_in_db.push(path.clone());
        }
    }

    result.modified.sort();
    result.only_in_db.sort();
    result.only_in_fs.sort();

    let mut summary_lines = vec![
        format!("Unchanged: {}", result.unchanged),
        format!("Modified: {}", result.modified.len()),
        format!("Only in DB: {}", result.only_in_db.len()),
        format!("Only in FS: {}", result.only_in_fs.len()),
    ];

    if let (Some(name), Some(stored_hash)) = (
        store.get_metadata("lockfile_name")?,
        store.get_metadata("lockfile_hash")?,
    ) {
        let lockfile_path = node_modules_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&name);
        let state = match std::fs::read_to_string(&lockfile_path) {
            Ok(content) if hash_string(&content) == stored_hash => "unchanged",
            Ok(_) => "CHANGED",
            Err(_) => "missing",
        };
        summary_lines.push(format!("Lockfile ({}): {}", name, state));
    }

    let sections: [(&str, char, &Vec<String>); 3] = [
        ("Modified files:", 'M', &result.modified),
        ("Only in DB (deleted locally):", 'D', &result.only_in_db),
        ("Only in FS (not in snapshot):", 'A', &result.only_in_fs),
    ];

    let mut truncated = false;
    for (title, marker, items) in sections {
        if items.is_empty() {
            continue;
        }
        summary_lines.push(String::new());
        summary_lines.push(title.to_string());
        for f in items.iter().take(10) {
            summary_lines.push(format!("  {} {}", marker, f));
        }
        if items.len() > 10 {
            summary_lines.push(format!("  ... and {} more", items.len() - 10));
            truncated = true;
        }
    }

    if truncated && !verbose {
        summary_lines.push(String::new());
        summary_lines.push("(Use --verbose for full list)".to_string());
    }

    let is_clean =
        result.modified.is_empty() && result.only_in_db.is_empty() && result.only_in_fs.is_empty();
    let color = if is_clean { "\x1b[32m" } else { "\x1b[33m" };

    let line_refs: Vec<&str> = summary_lines.iter().map(|s| s.as_str()).collect();
    print_box("Status", &line_refs, color);

    if verbose && truncated {
        for (_, marker, items) in [
            ("", 'M', &result.modified),
            ("", 'D', &result.only_in_db),
            ("", 'A', &result.only_in_fs),
        ] {
            for f in items {
                eprintln!("{} {}", marker, f);
            }
        }
    }

    if is_clean {
        eprintln!("All files match!");
    }

    Ok(result)
}
