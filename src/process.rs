use anyhow::Result;
use image::io::Reader;
use image::imageops;
use std::path::Path;

pub fn resize(source: &Path, dest: &Path, width: u32, height: u32) -> Result<()> {
    let image = Reader::open(&source)?.decode()?;
    let resized = imageops::resize(&image, width, height, imageops::FilterType::Lanczos3);
    Ok(resized.save(dest)?)
}
