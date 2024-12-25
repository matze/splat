use anyhow::{Context, Result};
use serde_derive::{Deserialize, Serialize};
use std::fs::{read_to_string, write};
use std::path::PathBuf;
use tera::Tera;

static CONFIG_TOML_FILENAME: &str = "splat.toml";

#[derive(Clone, Serialize, Deserialize)]
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
pub struct Theme {
    pub path: PathBuf,
    pub image_columns: usize,
    pub collection_columns: usize,
}

#[derive(Serialize, Deserialize)]
pub struct Toml {
    pub input: PathBuf,
    pub output: PathBuf,
    pub theme: Theme,
    pub thumbnail: Thumbnail,
    pub resize: Option<Resize>,
}

pub struct Config {
    pub toml: Toml,
    pub templates: Tera,
    pub static_path: Option<PathBuf>,
}

impl Config {
    pub fn new() -> Result<Self> {
        Config::from(Toml {
            input: PathBuf::from("input"),
            output: PathBuf::from("_build"),
            theme: Theme {
                path: PathBuf::from("theme"),
                image_columns: 4,
                collection_columns: 3,
            },
            thumbnail: Thumbnail {
                width: 450,
                height: 300,
            },
            resize: None,
        })
    }

    pub fn from(toml: Toml) -> Result<Self> {
        let theme_path = toml.theme.path.join("templates");
        let mut templates = tera::Tera::new(&theme_path.join("*.html").to_string_lossy())
            .context(format!("Could not load templates from {:?}", theme_path))?;

        templates.autoescape_on(vec![]);

        let static_path = toml.theme.path.join("static");

        Ok(Config {
            toml,
            templates,
            static_path: if static_path.exists() {
                Some(static_path)
            } else {
                None
            },
        })
    }

    pub fn read() -> Result<Self> {
        let toml: Toml = toml::from_str(
            &read_to_string(CONFIG_TOML_FILENAME)
                .context(format!("Could not open {}", CONFIG_TOML_FILENAME))?,
        )
        .context(format!("{} seem to be broken", CONFIG_TOML_FILENAME))?;

        Config::from(toml)
    }

    pub fn write(&self) -> Result<()> {
        Ok(write(CONFIG_TOML_FILENAME, toml::to_string(&self.toml)?)?)
    }
}
