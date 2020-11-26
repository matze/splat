use anyhow::{Context, Result};
use serde_derive::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

static CONFIG_TOML: &str = ".splat.toml";

#[derive(Serialize, Deserialize)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Resize {
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub input: PathBuf,
    pub output: PathBuf,
    pub thumbnail: Thumbnail,
    pub resize: Option<Resize>,
}

impl Config {
    pub fn new() -> Self {
        Config {
            input: PathBuf::from("."),
            output: PathBuf::from("_build"),
            thumbnail: Thumbnail {
                width: 450,
                height: 300
            },
            resize: None,
        }
    }

    pub fn read() -> Result<Self> {
        Ok(toml::from_str(&fs::read_to_string(CONFIG_TOML)
            .context(format!("Could not open {}", CONFIG_TOML))?)
            .context(format!("{} seem to be broken", CONFIG_TOML))?)
    }

    pub fn write(&self) -> Result<()> {
        Ok(fs::write(CONFIG_TOML, toml::to_string(&self)?)?)
    }
}
