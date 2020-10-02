use indicatif::{ProgressBar, ProgressStyle};

pub fn progress_bar(hidden: bool, message: &str, length: usize, unit: &str) -> ProgressBar {
    if hidden {
        ProgressBar::hidden()
    } else {
        let bar = ProgressBar::new(length as u64);

        bar.set_style(ProgressStyle::default_bar().template(&format!(
            "{{msg}} {{wide_bar}} {{pos}}/{{len}} {unit}",
            unit = unit
        )));

        bar.set_message(message);

        bar
    }
}
