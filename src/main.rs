mod config;
mod metadata;
mod process;

use anyhow::{anyhow, Result};
use config::Config;
use metadata::Metadata;
use process::{Operation, process};
use serde_derive::Serialize;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{create_dir_all, read_dir, write};
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use tera;

#[derive(StructOpt)]
#[structopt(name = "splat", about = "Static photo gallery generator")]
enum Commands {
    #[structopt(about = "Build static gallery")]
    Build,

    #[structopt(about = "Create new .splat.toml config")]
    New,
}

#[derive(Serialize)]
struct Item {
    path: PathBuf,
}

struct Collection {
    path: PathBuf,
    collections: Vec<Collection>,
    items: Vec<Item>,
    name: String,
    metadata: Option<Metadata>,
    thumbnail: PathBuf,
}

#[derive(Serialize)]
struct Output {
    title: String,
    items: Vec<Item>,
    thumbnail: PathBuf,
}

struct Builder {
    extensions: HashSet<OsString>,
    templates: Option<tera::Tera>,
    config: Config,
}

impl Builder {
    fn new(config: Config) -> Result<Self> {
        if !config.input.exists() {
            return Err(anyhow!("{:?} does not exist", config.input));
        }

        if !config.output.exists() {
            create_dir_all(&config.output)?;
        }

        let mut extensions = HashSet::new();
        extensions.insert(OsString::from("jpg"));

        let theme_path = Path::new("_theme/templates");

        if theme_path.exists() {
            let mut templates = tera::Tera::new("_theme/templates/*.html")?;

            // We disable autoescape because we will dump a lot of path-like strings which will have to
            // be marked as "safe" by the user.
            templates.autoescape_on(vec![]);

            Ok(Self {
                extensions: extensions,
                templates: Some(templates),
                config: config,
            })
        } else {
            Ok(Self {
                extensions: extensions,
                templates: None,
                config: config,
            })
        }
    }

    fn build(&self) -> Result<()> {
        match self.collect(&self.config.input)? {
            Some(collection) => self.process(collection, &self.config.output),
            None => Err(anyhow!("No images found")),
        }
    }

    fn collect(&self, root: &Path) -> Result<Option<Collection>> {
        let collections: Vec<Collection> = read_dir(root)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| self.collect(&entry.path()))
            .filter_map(Result::ok)
            .filter_map(|e| e)
            .collect();

        let items: Vec<Item> = read_dir(root)?
            .filter_map(Result::ok)
            .filter(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .map_or(false, |ext| self.extensions.contains(ext))
            })
            .map(|e| Item { path: e.path() })
            .collect();

        if items.is_empty() && collections.is_empty() {
            return Ok(None);
        }

        let metadata = Metadata::from_path(&root)?;

        // Determine thumbnail for this collection. We prioritize the one specified in the metadata
        // over the first item in this collection over the thumbnail of the first child collection.
        let thumbnail = metadata
            .as_ref()
            .map_or(None, |m| m.thumbnail.clone())
            .or_else(|| {
                items.first().map_or(
                    collections
                        .first()
                        .map_or(None, |c| Some(c.thumbnail.clone())),
                    |item| Some(item.path.clone()),
                )
            })
            .unwrap(); // TODO: try to get rid of

        Ok(Some(Collection {
            path: root.to_owned(),
            collections: collections,
            items: items,
            name: root.file_name().unwrap().to_string_lossy().to_string(),
            metadata: metadata,
            thumbnail: thumbnail,
        }))
    }

    fn process(&self, root: Collection, output: &Path) -> Result<()> {
        if !output.exists() {
            create_dir_all(output)?;
        }

        for child in root.collections {
            let output = output.join(child.path.file_name().ok_or(anyhow!("is .."))?);
            self.process(child, &output)?;
        }

        let thumb_dir = output.join("thumbnails");

        let ops: Vec<Operation> = root.items
            .iter()
            .map(|e| Operation::from(&e.path, output, &self.config))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|e| e)
            .collect();

        let items = ops
            .into_iter()
            .map(|op| process(op, &thumb_dir))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|path| Item { path: path })
            .collect();

        let thumbnail = self.config.output.join(
            root.thumbnail
                .strip_prefix(&&self.config.input)
                .unwrap()
                .parent()
                .unwrap()
                .join("thumbnails")
                .join(root.thumbnail.file_name().unwrap()),
        );

        let title = match &root.metadata {
            Some(metadata) => metadata.title.as_ref().unwrap().clone(),
            None => root.name.clone(),
        };

        if let Some(templates) = &self.templates {
            let mut context = tera::Context::new();

            context.insert(
                "collection",
                &Output {
                    title: title,
                    items: items,
                    thumbnail: thumbnail,
                },
            );

            let index_html = output.join("index.html");
            write(index_html, templates.render("index.html", &context)?)?;
        }

        Ok(())
    }
}

async fn build() -> Result<()> {
    Builder::new(Config::read().await?)?.build()
}

#[tokio::main]
async fn main() -> Result<()> {
    let commands = Commands::from_args();

    match commands {
        Commands::Build => build().await,
        Commands::New => Config::new().write().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image;
    use metadata::tests::METADATA;
    use std::fs::{copy, create_dir, write, File};
    use tempfile::{tempdir, TempDir};

    struct Fixture {
        builder: Builder,
        _dir: TempDir,
    }

    fn setup(resize: Option<(u32, u32)>) -> Result<Fixture> {
        let dir = tempdir()?;
        let input = dir.path().join("input");
        let output = dir.path().join("output");

        create_dir_all(&input)?;
        create_dir_all(&output)?;

        let config = config::Config {
            input: input,
            output: output,
            thumbnail: config::Thumbnail {
                width: 300,
                height: 200,
            },
            resize: resize.and_then(|r| Some(config::Resize {width: r.0, height: r.1})),
        };

        Ok(Fixture {
            builder: Builder::new(config)?,
            _dir: dir
        })
    }

    #[test]
    fn empty_dir_is_none_collection() -> Result<()> {
        let f = setup(None)?;
        let collection = f.builder.collect(&f.builder.config.input)?;
        assert!(collection.is_none());
        Ok(())
    }

    #[test]
    fn no_image_is_none_collection() -> Result<()> {
        let f = setup(None)?;
        File::create(f.builder.config.input.join("foo.bar"))?;
        assert!(f.builder.collect(&f.builder.config.input)?.is_none());
        Ok(())
    }

    #[test]
    fn single_image_is_some_collection() -> Result<()> {
        let f = setup(None)?;
        File::create(f.builder.config.input.join("test.jpg"))?;
        let collection = f.builder.collect(&f.builder.config.input)?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert_eq!(collection.items.len(), 1);
        Ok(())
    }

    #[test]
    fn choose_metadata_thumbnail() -> Result<()> {
        let f = setup(None)?;
        File::create(&f.builder.config.input.join("1.jpg"))?;
        File::create(&f.builder.config.input.join("2.jpg"))?;
        File::create(&f.builder.config.input.join("3.jpg"))?;
        write(f.builder.config.input.join("index.md"), "Thumbnail: 2.jpg")?;

        let collection = f.builder.collect(&f.builder.config.input)?.unwrap();
        assert_eq!(collection.thumbnail, f.builder.config.input.join("2.jpg"));
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.builder.config.input.join("test.jpg");
        File::create(&image_path)?;
        write(f.builder.config.input.join("index.md"), "Thumbnail: doesnotexist.jpg")?;

        let collection = f.builder.collect(&f.builder.config.input)?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail_on_conflict() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.builder.config.input.join("test.jpg");
        File::create(&image_path)?;
        write(f.builder.config.input.join("index.md"), "Thumbnail: doesnotexist.jpg")?;

        let collection = f.builder.collect(&f.builder.config.input)?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_subdir_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let subdir = f.builder.config.input.join("a");
        create_dir(&subdir)?;
        let image_path = subdir.join("test.jpg");
        File::create(&image_path)?;

        let collection = f.builder.collect(&f.builder.config.input)?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn single_image_in_subdir() -> Result<()> {
        let f = setup(None)?;
        let subdir = f.builder.config.input.join("a");
        create_dir(&subdir)?;
        File::create(subdir.join("test.jpg"))?;

        let collection = f.builder.collect(&f.builder.config.input)?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert_eq!(collection.items.len(), 0);
        assert_eq!(collection.collections.len(), 1);

        let child_collection = &collection.collections[0];
        assert_eq!(child_collection.items.len(), 1);
        assert_eq!(collection.thumbnail, child_collection.thumbnail);
        Ok(())
    }

    #[test]
    fn index_in_root_dir() -> Result<()> {
        let f = setup(None)?;

        File::create(f.builder.config.input.join("test.jpg"))?;

        write(f.builder.config.input.join("index.md"), METADATA)?;
        let collection = f.builder.collect(&f.builder.config.input)?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert!(collection.metadata.is_some());
        Ok(())
    }

    #[test]
    fn process_copy() -> Result<()> {
        let f = setup(None)?;

        // Copy test.jpg, which is 900x600 pixels to the root input dir.
        copy("data/test.jpg", f.builder.config.input.join("test.jpg"))?;

        f.builder.build()?;
        let copy_name = f.builder.config.output.join("test.jpg");
        let thumb_name = f.builder.config.output.join("thumbnails/test.jpg");

        assert!(copy_name.exists());
        assert!(thumb_name.exists());

        let copy_dims = image::image_dimensions(copy_name)?;
        assert_eq!(copy_dims, (900, 600));

        let thumb_dims = image::image_dimensions(thumb_name)?;
        assert_eq!(thumb_dims, (300, 200));

        Ok(())
    }

    #[test]
    fn process_resize() -> Result<()> {
        let f = setup(Some((600, 400)))?;
        // Copy test.jpg, which is 900x600 pixels to the root input dir.
        copy("data/test.jpg", f.builder.config.input.join("test.jpg"))?;

        f.builder.build()?;
        let copy_name = f.builder.config.output.join("test.jpg");
        let thumb_name = f.builder.config.output.join("thumbnails/test.jpg");

        assert!(copy_name.exists());
        assert!(thumb_name.exists());

        let copy_dims = image::image_dimensions(copy_name)?;
        assert_eq!(copy_dims, (600, 400));

        let thumb_dims = image::image_dimensions(thumb_name)?;
        assert_eq!(thumb_dims, (300, 200));

        Ok(())
    }
}
