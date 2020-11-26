use anyhow::Result;
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Metadata {
    pub description: String,
    pub title: Option<String>,
    pub thumbnail: Option<PathBuf>,
}

fn from_str(path: &Path, content: &str) -> Result<Metadata> {
    let re = Regex::new(r"([[:alpha:]]+): (.+)")?;
    let mut lines = content.lines();

    let mut keys = lines
        .by_ref()
        .take_while(|&v| re.is_match(v))
        .filter_map(|v| re.captures(v))
        .map(|caps| (caps[1].to_string(), caps[2].to_string()))
        .collect::<HashMap<_, _>>();

    let description = lines
        .skip_while(|&line| line == "\n")
        .fold(String::new(), |a, b| a + "\n" + b)
        .trim_start()
        .to_owned();

    let thumbnail = keys
        .remove("Thumbnail")
        .map_or(None, |s| Some(path.join(PathBuf::from(s))))
        .filter(|path| path.exists());

    Ok(Metadata {
        description: description,
        title: keys.remove("Title"),
        thumbnail: thumbnail,
    })
}

impl Metadata {
    pub fn from_path(root: &Path) -> Result<Option<Metadata>> {
        let index = root.join("index.md");

        if !index.exists() {
            return Ok(None);
        }

        let mut file = File::open(index)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(Some(from_str(root, &contents)?))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub static METADATA: &str = "Title: foo\n\nDescription.\n\nNext paragraph.";

    #[test]
    fn parse_metadata() -> Result<()> {
        let metadata = from_str(&PathBuf::from("."), METADATA)?;
        assert_eq!(metadata.title.unwrap(), "foo");
        assert_eq!(metadata.description, "Description.\n\nNext paragraph.");
        Ok(())
    }
}
