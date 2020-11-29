use crate::config;
use anyhow::{anyhow, Result};
use image::imageops;
use image::io::Reader;
use std::fs::{copy, create_dir_all};
use std::path::{Path, PathBuf};

pub struct Pair {
    from: PathBuf,
    to: PathBuf,
    thumbnail: config::Thumbnail,
}

pub struct Resize {
    pair: Pair,
    width: u32,
    height: u32,
}

pub enum Operation {
    Copy(Pair),
    Resize(Resize),
}

fn resize(source: &Path, dest: &Path, width: u32, height: u32) -> Result<()> {
    let image = Reader::open(&source)?.decode()?;
    let resized = imageops::resize(&image, width, height, imageops::FilterType::Lanczos3);
    Ok(resized.save(dest)?)
}

fn is_older(first: &Path, second: &Path) -> Result<bool> {
    Ok(first.metadata()?.modified()? < second.metadata()?.modified()?)
}

fn generate_thumbnail(operation: &Operation, thumb_dir: &Path) -> Result<()> {
    if !thumb_dir.exists() {
        create_dir_all(&thumb_dir)?;
    }

    let (from, thumbnail) = match operation {
        Operation::Copy(ref pair) => (&pair.from, &pair.thumbnail),
        Operation::Resize(ref output) => (&output.pair.from, &output.pair.thumbnail),
    };

    let file_name = from.file_name().ok_or(anyhow!("not a file"))?;
    let thumb_path = thumb_dir.join(file_name);

    if !thumb_path.exists() || is_older(&thumb_path, from)? {
        resize(from, &thumb_path, thumbnail.width, thumbnail.height)?;
    }

    Ok(())
}

pub fn process(operation: Operation, thumb_dir: &Path) -> Result<PathBuf> {
    generate_thumbnail(&operation, thumb_dir)?;

    match operation {
        Operation::Copy(pair) => {
            copy(&pair.from, &pair.to)?;
            Ok(pair.to)
        }
        Operation::Resize(output) => {
            resize(
                &output.pair.from,
                &output.pair.to,
                output.width,
                output.height,
            )?;

            Ok(output.pair.to)
        }
    }
}

impl Operation {
    pub fn from(image_path: &Path, output: &Path, config: &config::Config) -> Result<Option<Self>> {
        let file_name = image_path.file_name().ok_or(anyhow!("not a file"))?;
        let dest_path = output.join(file_name);

        if !dest_path.exists() || is_older(&dest_path, &image_path)? {
            let pair = Pair {
                from: image_path.to_owned(),
                to: dest_path,
                thumbnail: config.thumbnail.clone(),
            };

            if let Some(target) = &config.resize {
                return Ok(Some(Operation::Resize(Resize {
                    pair: pair,
                    width: target.width,
                    height: target.height,
                })));
            } else {
                return Ok(Some(Operation::Copy(pair)));
            }
        }

        Ok(None)
    }
}
