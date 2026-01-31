use indicatif::{ProgressBar, ProgressStyle};

pub fn create_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "[{bar:30.cyan/dim}] {percent}% ({pos}/{len}) {elapsed_precise} - {msg}",
        )
        .unwrap()
        .progress_chars("█░░"),
    );
    pb
}
