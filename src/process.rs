use crate::config;
use crate::Item;
use anyhow::{anyhow, Result};
use image::imageops;
use image::io::Reader;
use std::fs::{copy, create_dir_all};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

pub struct Process<'a> {
    pub config: &'a config::Config,
    pub item: &'a Item,
    pub sender: Sender<Result<()>>,
}

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

fn generate_thumbnail(p: &Process) -> Result<()> {
    let thumb_dir = p.config
        .output
        .join(
            p.item.from
                .parent()
                .unwrap()
                .strip_prefix(&p.config.input)
                .unwrap(),
        )
        .join("thumbnails");

    if !thumb_dir.exists() {
        create_dir_all(&thumb_dir)?;
    }

    let thumb_path = thumb_dir.join(p.item.from.file_name().unwrap());

    if !thumb_path.exists() || is_older(&thumb_path, &p.item.from)? {
        resize(
            &p.item.from,
            &thumb_path,
            p.config.thumbnail.width,
            p.config.thumbnail.height,
        )?;
    }

    Ok(())
}

pub fn process(p: Process) {
    let result = generate_thumbnail(&p);

    if result.is_err() {
        p.sender.send(result).unwrap();
        return;
    }

    let result = match &p.config.resize {
        Some(target) => {
            resize(&p.item.from, &p.item.to, target.width, target.height)
        },
        None => match copy(&p.item.from, &p.item.to) {
            Err(e) => Err(anyhow!("Copying {:?} => {:?} failed: {}", p.item.from, p.item.to, e)),
            Ok(_) => Ok(())
        }
    };

    p.sender.send(result).unwrap();
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
