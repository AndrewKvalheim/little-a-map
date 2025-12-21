use crate::palette::PALETTE;
use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use fs_err::File;
use indicatif::{ProgressBar, ProgressStyle};
use std::array;
use std::borrow::Cow;
use std::io::{Read, Write};
use std::path::Path;
use std::time::SystemTime;

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
    let mut decoder = GzDecoder::new(File::open(path)?);
    let mut data = Vec::new();

    decoder.read_to_end(&mut data)?;

    Ok(data)
}

pub fn write_webp(w: &mut impl Write, indexed: &[u8; 128 * 128]) -> Result<()> {
    let rgb: [u8; 128 * 128 * 3] = array::from_fn(|i| PALETTE[indexed[i / 3] as usize * 3 + i % 3]);
    let encoder = webp::Encoder::from_rgb(&rgb, 128, 128);
    let encoded = encoder
        .encode_simple(true, 100.0)
        .map_err(|e| anyhow!("WebP encoding error: {e:?}"))?;
    w.write_all(&encoded)?;

    Ok(())
}

/// Wraps [`fs_err::File`] because it does not yet have a [`std::fs::File::set_modified`] method.
pub fn set_modified(file: &File, modified: SystemTime) -> Result<()> {
    let path = file.path();
    let display = path.display().to_string();
    file.file()
        .set_modified(modified)
        .context(format!("failed to set modified time for file: `{display}`"))
}
