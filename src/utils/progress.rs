use indicatif::{ProgressBar, ProgressStyle};

pub fn truncate_message(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_ascii() {
        assert_eq!(truncate_message("hello", 40), "hello");
        assert_eq!(truncate_message("hello", 3), "hel");
    }

    #[test]
    fn test_truncate_multibyte() {
        assert_eq!(truncate_message("한글파일명", 3), "한글파");
        assert_eq!(truncate_message("한글", 40), "한글");
    }
}

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
