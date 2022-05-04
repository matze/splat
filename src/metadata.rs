use anyhow::Result;
use pulldown_cmark::{html, Parser};
use regex::Regex;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Metadata {
    pub description: String,
    pub title: String,
    pub thumbnail: Option<PathBuf>,
}

fn from_str(path: &Path, content: &str) -> Result<Metadata> {
    let re = Regex::new(r"([[:alpha:]]+): (.+)")?;
    let lines = content.lines();
    let mut matching_phase = true;
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut description = String::new();

    for line in lines {
        if matching_phase {
            if re.is_match(line) {
                let caps = re.captures(line).unwrap();
                keys.insert(caps[1].to_string(), caps[2].to_string());
                continue;
            } else {
                matching_phase = false;
            }
        }

        description.push_str(line);
        description.push('\n');
    }

    let parser = Parser::new(&description);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    let thumbnail = keys
        .remove("Thumbnail")
        .map(|s| path.join(PathBuf::from(s)))
        .filter(|path| path.exists());

    let title = keys.remove("Title").unwrap_or_else(|| {
        path.file_name()
            .unwrap_or(&OsString::new())
            .to_str()
            .unwrap()
            .to_owned()
    });

    Ok(Metadata {
        description: html_output,
        title,
        thumbnail,
    })
}

impl Metadata {
    pub fn from_path(root: &Path) -> Result<Metadata> {
        let index = root.join("index.md");

        if !index.exists() {
            return Ok(Metadata {
                description: "".to_string(),
                title: root.file_name().unwrap().to_str().unwrap().to_owned(),
                thumbnail: None,
            });
        }

        let mut file = File::open(index)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        from_str(root, &contents)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub static METADATA: &str = "Title: foo\n\nDescription.\n\nNext paragraph.";

    #[test]
    fn parse_metadata() -> Result<()> {
        let metadata = from_str(&PathBuf::from("."), METADATA)?;
        assert_eq!(metadata.title, "foo");
        assert_eq!(
            metadata.description,
            "<p>Description.</p>\n<p>Next paragraph.</p>\n"
        );
        Ok(())
    }

    #[test]
    fn no_metadata_is_description() -> Result<()> {
        let metadata = from_str(&PathBuf::from("."), "This is *bold*.")?;
        assert_eq!(metadata.title, "");
        assert_eq!(metadata.description, "<p>This is <em>bold</em>.</p>\n");
        Ok(())
    }
}
