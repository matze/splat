use crate::process::is_older;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::read_to_string;
use std::path::PathBuf;
use tera::Tera;

pub static TOML_FILENAME: &str = "splat.toml";

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

/// Generate `output` from `input` via the `command` which must contain Makefile style $@ and $< to
/// reference them. `output` is only re-generated when older than `input`.
#[derive(Serialize, Deserialize, Debug)]
pub struct Process {
    /// Input path passed to the `command`.
    input: PathBuf,
    /// Output path passed to the `command`.
    output: PathBuf,
    /// Command to execute to generate `output`, expands $@ to `output` and $< to `input`.
    command: String,
}

#[derive(Serialize, Deserialize)]
pub struct Theme {
    pub path: PathBuf,
    pub process: Option<Vec<Process>>,
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
    pub fn read() -> Result<Self> {
        let toml: Toml = toml::from_str(
            &read_to_string(TOML_FILENAME).context(format!("Could not open {}", TOML_FILENAME))?,
        )
        .context(format!("{} seem to be broken", TOML_FILENAME))?;

        Config::try_from(toml)
    }
}

impl TryFrom<Toml> for Config {
    type Error = anyhow::Error;

    fn try_from(toml: Toml) -> Result<Self, Self::Error> {
        let theme_path = toml.theme.path.join("templates");
        let mut templates = tera::Tera::new(&theme_path.join("*.html").to_string_lossy())
            .context(format!("Could not load templates from {:?}", theme_path))?;

        templates.autoescape_on(vec![]);

        let static_path = toml.theme.path.join("static");
        let static_path = static_path.exists().then_some(static_path);

        Ok(Config {
            toml,
            templates,
            static_path,
        })
    }
}

impl Process {
    /// Expand $< and $@ and run the command.
    pub fn run(&self) -> Result<()> {
        if self.output.exists() && !is_older(&self.output, &self.input)? {
            return Ok(());
        }

        let input = self.input.as_os_str();
        let output = self.output.as_os_str();
        let mut split = self.command.split(' ');

        let program = split.next().ok_or_else(|| anyhow!("no program given"))?;

        let args = split
            .map(|part| match part {
                "$<" => input,
                "$@" => output,
                part => OsStr::new(part),
            })
            .collect::<Vec<_>>();

        print!("  Running {program} ...");
        let output = std::process::Command::new(program).args(args).output()?;

        if output.status.success() {
            println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m {program} finished successfully");
        }

        Ok(())
    }
}
