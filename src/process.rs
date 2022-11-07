use crate::config;
use crate::Item;
use anyhow::{anyhow, Context, Result};
use image::imageops;
use image::io::Reader;
use std::fs::{copy, create_dir_all};
use std::path::Path;
use std::sync::mpsc::Sender;

pub struct Process<'a> {
    pub config: &'a config::Config,
    pub item: &'a Item,
    pub sender: Sender<Result<()>>,
}

fn resize(source: &Path, dest: &Path, width: u32, height: u32) -> Result<()> {
    let image = Reader::open(source)?
        .decode()
        .context(format!("{:?} does not seem to be a valid image", source))?;
    let resized = image.resize_to_fill(width, height, imageops::FilterType::Lanczos3);
    Ok(resized.save(dest)?)
}

pub fn is_older(first: &Path, second: &Path) -> Result<bool> {
    Ok(first.metadata()?.modified()? < second.metadata()?.modified()?)
}

fn generate_thumbnail(p: &Process) -> Result<()> {
    let thumb_dir = p.item.thumbnail.parent();

    if let Some(dir) = thumb_dir {
        if !dir.exists() {
            create_dir_all(dir)?;
        }
    }

    if !p.item.thumbnail.exists() || p.item.thumbnail_outdated()? {
        resize(
            &p.item.from,
            &p.item.thumbnail,
            p.config.toml.thumbnail.width,
            p.config.toml.thumbnail.height,
        )?;
    }

    Ok(())
}

fn wrapped_process(p: &Process) -> Result<()> {
    generate_thumbnail(p)?;

    if p.item.to.exists() && is_older(&p.item.to, &p.item.from)? {
        return Ok(());
    }

    match &p.config.toml.resize {
        Some(target) => resize(&p.item.from, &p.item.to, target.width, target.height),
        None => copy(&p.item.from, &p.item.to)
            .context(format!("Copying {:?} => {:?}", p.item.from, p.item.to))
            .map(|_| ()),
    }?;

    Ok(())
}

pub fn process(p: &Process) {
    p.sender.send(wrapped_process(p)).unwrap();
}

fn do_copy(path: &Path, prefix: &Path, output: &Path) -> Result<()> {
    for item in path.read_dir()? {
        let path = item?.path();
        let dest = output.join(path.strip_prefix(prefix)?);

        if path.is_dir() {
            create_dir_all(dest)?;
            do_copy(&path, prefix, output)?;
        } else if !dest.exists() || is_older(&dest, &path)? {
            copy(&path, dest)?;
        }
    }

    Ok(())
}

pub fn copy_recursively(path: &Path, output: &Path) -> Result<()> {
    let prefix = path.parent().ok_or_else(|| anyhow!("No parent"))?;
    do_copy(path, prefix, output)
}
