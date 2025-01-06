use anyhow::Result;
use pulldown_cmark::{html, Parser};
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Metadata read from `index.md`s at the root of a collection's directory.
pub struct Metadata {
    /// Free text description.
    pub description: String,
    /// Override title defaulting to a collections directory name else.
    pub title: String,
    /// Override thumbnail image to use.
    pub thumbnail: Option<PathBuf>,
}

static EXPRESSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([[:alpha:]]+): (.+)").expect("constructing regex"));

fn path_to_string(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_owned()
}

fn from_str(path: &Path, content: &str) -> Result<Metadata> {
    let lines = content.lines();
    let mut matching_phase = true;
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut description = String::new();

    for line in lines {
        if matching_phase {
            if let Some(caps) = EXPRESSION.captures(line) {
                keys.insert(caps[1].to_string(), caps[2].to_string());
                continue;
            }

            matching_phase = false;
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

    let title = keys.remove("Title").unwrap_or_else(|| path_to_string(path));

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
                description: String::new(),
                title: path_to_string(root),
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
