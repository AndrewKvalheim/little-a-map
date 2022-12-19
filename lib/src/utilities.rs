use anyhow::Result;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use std::borrow::Cow;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn progress_bar(
    quiet: bool,
    message: impl Into<Cow<'static, str>>,
    total: usize,
    unit: &str,
) -> ProgressBar {
    if quiet {
        ProgressBar::hidden()
    } else {
        let bar = ProgressBar::new(total as u64);

        bar.set_style(
            ProgressStyle::with_template(&format!(
                "{{msg}} {{wide_bar}} {{human_pos}}/{{human_len}} {unit}"
            ))
            .unwrap(),
        );

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
