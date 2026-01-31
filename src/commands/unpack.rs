use anyhow::{bail, Result};
use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::commands::pack::print_box;
use crate::core::extractor::extract_files_parallel;
use crate::core::store::Store;
use crate::types::UnpackOptions;
use crate::utils::fs::format_bytes;
use crate::utils::progress::create_progress_bar;

pub fn unpack(options: &UnpackOptions) -> Result<()> {
    let db_path = Path::new(&options.input);
    let output_path = Path::new(&options.output);

    if !db_path.exists() {
        bail!("Database not found: {}", db_path.display());
    }

    if output_path.exists() {
        if !options.force {
            bail!(
                "Output directory already exists: {}. Use --force to overwrite.",
                output_path.display()
            );
        }

        eprintln!("Removing existing {}...", output_path.display());
        fs::remove_dir_all(output_path)?;
    }

    eprintln!("Opening {}", db_path.display());
    let store = Store::open(db_path.to_str().unwrap_or_default())?;

    let created_at = store
        .get_metadata("created_at")?
        .unwrap_or_else(|| "unknown".to_string());
    let total_file_count = store.get_total_file_count()?;
    let blob_stats = store.get_blob_stats()?;

    print_box(
        "Database Info",
        &[
            &format!("Created: {}", created_at),
            &format!("Files: {}", total_file_count),
            &format!("Original size: {}", format_bytes(blob_stats.total_original_size)),
            &format!(
                "Compressed size: {}",
                format_bytes(blob_stats.total_compressed_size)
            ),
        ],
        "\x1b[36m",
    );

    eprintln!("Extracting to {}", output_path.display());
    let pb = create_progress_bar(total_file_count as u64);

    let start = Instant::now();
    let (total_files, total_size) = extract_files_parallel(&store, output_path, Some(&|current, total, msg| {
        pb.set_length(total as u64);
        pb.set_position(current as u64);
        pb.set_message(msg.to_string());
    }))?;
    let elapsed = start.elapsed().as_secs_f64();
    pb.finish_and_clear();

    print_box(
        "Unpack Complete",
        &[
            &format!("Extracted: {} files ({})", total_files, format_bytes(total_size)),
            &format!("Time: {:.1}s", elapsed),
        ],
        "\x1b[32m",
    );

    Ok(())
}
