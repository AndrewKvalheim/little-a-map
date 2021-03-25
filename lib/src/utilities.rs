use anyhow::Result;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::Read;
use std::path::Path;

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

pub fn read_gz(path: &Path) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(File::open(&path)?);
    let mut data = Vec::new();

    decoder.read_to_end(&mut data)?;

    Ok(data)
}
