use crate::config;
use crate::Item;
use anyhow::Result;
use image::imageops;
use image::io::Reader;
use std::fs::{copy, create_dir_all};
use std::path::Path;

fn resize(source: &Path, dest: &Path, width: u32, height: u32) -> Result<()> {
    let image = Reader::open(&source)?.decode()?;
    let resized = imageops::resize(&image, width, height, imageops::FilterType::Lanczos3);
    Ok(resized.save(dest)?)
}

fn is_older(first: &Path, second: &Path) -> Result<bool> {
    Ok(first.metadata()?.modified()? < second.metadata()?.modified()?)
}

fn generate_thumbnail(item: &Item, config: &config::Config) -> Result<()> {
    let thumb_dir = config
        .output
        .join(
            item.from
                .parent()
                .unwrap()
                .strip_prefix(&config.input)
                .unwrap(),
        )
        .join("thumbnails");

    if !thumb_dir.exists() {
        create_dir_all(&thumb_dir)?;
    }

    let thumb_path = thumb_dir.join(item.from.file_name().unwrap());

    if !thumb_path.exists() || is_older(&thumb_path, &item.from)? {
        resize(
            &item.from,
            &thumb_path,
            config.thumbnail.width,
            config.thumbnail.height,
        )?;
    }

    Ok(())
}

pub fn process(item: &Item, config: &config::Config) -> Result<()> {
    generate_thumbnail(item, config)?;

    if !item.to.exists() || is_older(&item.to, &item.from)? {
        if let Some(target) = &config.resize {
            resize(&item.from, &item.to, target.width, target.height)?;
        } else {
            copy(&item.from, &item.to)?;
        }
    }
    Ok(())
}
