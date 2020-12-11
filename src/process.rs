use crate::config;
use crate::Item;
use anyhow::Result;
use image::imageops;
use image::io::Reader;
use std::fs::{copy, create_dir_all};
use std::path::{Path, PathBuf};

fn resize(source: &Path, dest: &Path, width: u32, height: u32) -> Result<()> {
    let source = source.to_owned();
    let dest = dest.to_owned();
    let image = Reader::open(&source)?.decode()?;
    let resized = image.resize_to_fill(width, height, imageops::FilterType::Lanczos3);
    Ok(resized.save(dest)?)
}

pub fn is_older(first: &Path, second: &Path) -> Result<bool> {
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

    if let Some(target) = &config.resize {
        resize(&item.from, &item.to, target.width, target.height)?;
    } else {
        copy(&item.from, &item.to)?;
    }

    Ok(())
}

fn do_copy(path: &Path, prefix: &Path, output: &Path) -> Result<()> {
    for item in path.read_dir()? {
        let path = item?.path();
        let dest = output.join(path.strip_prefix(prefix)?);

        if path.is_dir() {
            create_dir_all(dest)?;
            do_copy(&path, prefix, output)?;
        } else {
            if !dest.exists() || is_older(&dest, &path)? {
                copy(&path, dest)?;
            }
        }
    }

    Ok(())
}

pub fn copy_recursively(path: &PathBuf, output: &Path) -> Result<()> {
    let prefix = &path.parent().unwrap();
    Ok(do_copy(path, &prefix, output)?)
}
