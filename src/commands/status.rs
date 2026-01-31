use anyhow::{bail, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use crate::commands::pack::print_box;
use crate::core::hasher::hash_buffer;
use crate::core::store::Store;
use crate::types::StatusResult;
use crate::utils::progress::create_progress_bar;

pub fn status(db: &str, node_modules: &str) -> Result<StatusResult> {
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

    let store = Store::open(db_path.to_str().unwrap_or_default())?;
    let files = store.get_all_files()?;
    let total = files.len();

    let pb = create_progress_bar(total as u64);

    let result = Mutex::new(StatusResult::default());
    let db_paths = Mutex::new(HashSet::new());

    files.par_iter().enumerate().for_each(|(index, file)| {
        let relative_path = format!("{}/{}", file.package_path, file.record.relative_path);
        let full_path = node_modules_path.join(&relative_path);

        db_paths.lock().unwrap().insert(relative_path.clone());

        pb.set_position((index + 1) as u64);
        if file.record.relative_path.len() > 40 {
            pb.set_message(file.record.relative_path[..40].to_string());
        } else {
            pb.set_message(file.record.relative_path.clone());
        }

        if !full_path.exists() {
            result.lock().unwrap().only_in_db.push(relative_path);
            return;
        }

        match std::fs::read(&full_path) {
            Ok(content) => {
                let fs_hash = hash_buffer(&content);
                if fs_hash != file.record.blob_hash {
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

    let result = result.into_inner().unwrap();

    let mut summary_lines = vec![
        format!("Unchanged: {}", result.unchanged),
        format!("Modified: {}", result.modified.len()),
        format!("Only in DB: {}", result.only_in_db.len()),
    ];

    if !result.modified.is_empty() && result.modified.len() <= 10 {
        summary_lines.push(String::new());
        summary_lines.push("Modified files:".to_string());
        for f in &result.modified {
            summary_lines.push(format!("  M {}", f));
        }
    }

    if !result.only_in_db.is_empty() && result.only_in_db.len() <= 10 {
        summary_lines.push(String::new());
        summary_lines.push("Only in DB (deleted locally):".to_string());
        for f in &result.only_in_db {
            summary_lines.push(format!("  D {}", f));
        }
    }

    if result.modified.len() > 10 || result.only_in_db.len() > 10 {
        summary_lines.push(String::new());
        summary_lines.push("(Use verbose mode for full list)".to_string());
    }

    let is_clean = result.modified.is_empty() && result.only_in_db.is_empty();
    let color = if is_clean { "\x1b[32m" } else { "\x1b[33m" };

    let line_refs: Vec<&str> = summary_lines.iter().map(|s| s.as_str()).collect();
    print_box("Status", &line_refs, color);

    if is_clean {
        eprintln!("All files match!");
    }

    Ok(result)
}
