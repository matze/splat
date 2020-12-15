#[macro_use]
extern crate lazy_static;

mod config;
mod metadata;
mod process;

use anyhow::{anyhow, Result};
use config::Config;
use metadata::Metadata;
use process::{copy_recursively, is_older, process, Process};
use rayon::prelude::*;
use serde_derive::Serialize;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{create_dir_all, read_dir, write};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::thread;
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

pub struct Item {
    from: PathBuf,
    to: PathBuf,
}

struct Collection {
    path: PathBuf,
    collections: Vec<Collection>,
    items: Vec<Item>,
    metadata: Metadata,
    thumbnail: PathBuf,
}

#[derive(Clone, Serialize)]
struct Image {
    path: String,
    thumbnail: String,
    width: u32,
    height: u32,
}

#[derive(Clone, Serialize)]
struct Child<'a> {
    path: String,
    thumbnail: String,
    title: &'a str,
}

#[derive(Serialize)]
struct Link {
    title: String,
    path: String,
}

#[derive(Serialize)]
struct Output<'a> {
    title: &'a str,
    description: &'a str,
    breadcrumbs: Vec<Link>,
    children: Vec<Vec<Child<'a>>>,
    rows: Vec<Vec<Image>>,
}

struct Builder {
    config: Config,
}

lazy_static! {
    static ref EXTENSIONS: HashSet<OsString> = {
        let mut extensions = HashSet::new();
        extensions.insert(OsString::from("jpg"));
        extensions
    };
}

fn rowify<T: Clone>(items: Vec<T>, num_columns: usize) -> Vec<Vec<T>> {
    items
        .chunks(num_columns)
        .into_iter()
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn breadcrumbs_to_links(breadcrumbs: &Vec<String>) -> Vec<Link> {
    let mut path = ".".to_owned();
    let mut links = Vec::new();

    for breadcrumb in breadcrumbs.iter().rev() {
        links.push(Link {
            title: breadcrumb.clone(),
            path: path.clone(),
        });
        path = format!("{}/..", path);
    }

    links
}

fn output_path_to_root(output: &Path) -> PathBuf {
    let mut path = PathBuf::new();

    for _ in 1..output.iter().count() {
        path = path.join("..");
    }

    path
}

impl Image {
    fn from(item: &Item) -> Result<Self> {
        let file_name = item.to.file_name().unwrap().to_string_lossy().into_owned();
        let dims = image::image_dimensions(&item.to)?;

        Ok(Self {
            thumbnail: format!("thumbnails/{}", &file_name),
            path: file_name,
            width: dims.0,
            height: dims.1,
        })
    }
}

impl Item {
    fn needs_update(&self) -> bool {
        !self.to.exists() || is_older(&self.to, &self.from).unwrap()
    }
}

impl<'a> Child<'a> {
    fn from(collection: &'a Collection) -> Result<Self> {
        // TODO: yo, fix this mess ...
        let dir_name = collection
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let thumb_dir = collection
            .thumbnail
            .strip_prefix(&collection.path.parent().unwrap())?;
        let thumb_filename = thumb_dir
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let thumb_path = thumb_dir
            .parent()
            .unwrap()
            .join("thumbnails")
            .join(thumb_filename)
            .to_string_lossy()
            .into_owned();

        Ok(Self {
            thumbnail: thumb_path,
            path: dir_name,
            title: &collection.metadata.title,
        })
    }
}

impl Collection {
    fn from(current: &Path, output: &Path, config: &Config) -> Result<Option<Self>> {
        let collections: Vec<Collection> = read_dir(current)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| Collection::from(&entry.path(), output, config))
            .filter_map(Result::ok)
            .filter_map(|e| e)
            .collect();

        let items: Vec<Item> = read_dir(current)?
            .filter_map(Result::ok)
            .filter(|e| {
                e.path().is_file()
                    && e.path()
                        .extension()
                        .map_or(false, |ext| EXTENSIONS.contains(ext))
            })
            .map(|e| Item {
                from: e.path(),
                to: config
                    .toml
                    .output
                    .join(e.path().strip_prefix(&config.toml.input).unwrap()),
            })
            .collect();

        if items.is_empty() && collections.is_empty() {
            return Ok(None);
        }

        let metadata = Metadata::from_path(&current)?;

        // Determine thumbnail for this collection. We prioritize the one specified in the metadata
        // over the first item in this collection over the thumbnail of the first child collection.
        let thumbnail = metadata
            .thumbnail
            .as_ref()
            .map_or(None, |thumbnail| Some(thumbnail.clone()))
            .or_else(|| {
                items.first().map_or(
                    collections
                        .first()
                        .map_or(None, |c| Some(c.thumbnail.clone())),
                    |item| Some(item.from.clone()),
                )
            })
            .unwrap(); // TODO: try to get rid of

        Ok(Some(Collection {
            path: current.to_owned(),
            collections: collections,
            items: items,
            metadata: metadata,
            thumbnail: thumbnail,
        }))
    }

    /// Return all items from this and all sub collections.
    fn items(&self) -> Vec<&Item> {
        let mut items: Vec<_> = self.items.iter().collect();

        for child in &self.collections {
            items.extend(child.items());
        }

        items
    }
}

impl Builder {
    fn new(config: Config) -> Result<Self> {
        if !config.toml.input.exists() {
            return Err(anyhow!("{:?} does not exist", config.toml.input));
        }

        if !config.toml.output.exists() {
            create_dir_all(&config.toml.output)?;
        }

        Ok(Self { config: config })
    }

    fn build(&self) -> Result<()> {
        if let Some(static_path) = self.config.static_path.as_ref() {
            print!("  Copying static data ...");
            copy_recursively(&static_path, &self.config.toml.output)?;
            println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Copied static data");
        }

        let collection = Collection::from(
            &self.config.toml.input,
            &self.config.toml.output,
            &self.config,
        )?;

        if collection.is_none() {
            return Err(anyhow!("No images found"));
        }

        let collection = collection.unwrap();

        let items = collection
            .items()
            .into_iter()
            .filter(|item| item.needs_update())
            .collect::<Vec<_>>();

        let num_items = items.len();
        let (sender, receiver) = channel::<Result<()>>();

        let processes = items
            .into_iter()
            .map(|item| Process {
                config: &self.config,
                item: &item,
                sender: sender.clone(),
            })
            .collect::<Vec<_>>();

        thread::spawn(move || {
            let spinners = vec!["⠖", "⠲", "⠴", "⠦"];
            let num_spinners = spinners.len();

            for i in 0..num_items {
                print!(
                    "\x1B[2K\r\x1B[0;36m{}\x1B[0;m Processing {} images ...",
                    spinners[i % num_spinners],
                    num_items - i
                );
                io::stdout().flush().unwrap();

                let result = receiver.recv();

                if result.is_err() {
                    println!("\rError: {:?}", result);
                }
            }

            println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Processed {} images", num_items);
        });

        processes.into_par_iter().for_each(|p| process(p));

        print!("  Writing HTML pages ...");
        let mut breadcrumbs: Vec<String> = Vec::new();
        self.write_html(&collection, &mut breadcrumbs, &self.config.toml.output)?;
        println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Wrote HTML pages");

        Ok(())
    }

    fn write_html(
        &self,
        collection: &Collection,
        breadcrumbs: &mut Vec<String>,
        output: &Path,
    ) -> Result<()> {
        if !output.exists() {
            create_dir_all(output)?;
        }

        for child in &collection.collections {
            let subdir = child.path.file_name().unwrap();
            let output = output.join(subdir);

            breadcrumbs.push(subdir.to_string_lossy().to_string());
            self.write_html(child, breadcrumbs, &output)?;
            breadcrumbs.remove(breadcrumbs.len() - 1);
        }

        let items = collection
            .items
            .iter()
            .map(|item| Image::from(&item))
            .collect::<Result<Vec<_>, _>>()?;

        let mut children = collection
            .collections
            .iter()
            .map(|collection| Child::from(&collection))
            .collect::<Result<Vec<_>, _>>()?;

        children.sort_by(|a, b| b.title.cmp(&a.title));

        let mut context = tera::Context::new();
        let links = breadcrumbs_to_links(&breadcrumbs);

        context.insert(
            "collection",
            &Output {
                title: &collection.metadata.title,
                description: &collection.metadata.description,
                breadcrumbs: links,
                children: rowify(children, self.config.toml.theme.collection_columns),
                rows: rowify(items, self.config.toml.theme.image_columns),
            },
        );

        let static_path = output_path_to_root(&output).join("static");
        context.insert("theme_url", &static_path);

        let index_html = output.join("index.html");

        Ok(write(
            index_html,
            self.config.templates.render("index.html", &context)?,
        )?)
    }
}

fn build() -> Result<()> {
    Builder::new(Config::read()?)?.build()
}

fn main() {
    let commands = Commands::from_args();

    let result = match commands {
        Commands::Build => build(),
        Commands::New => Config::new().and_then(|config| config.write()),
    };

    if let Err(err) = result {
        println!("\x1B[2K\r\x1B[0;31m! {}\x1B[0;m", err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image;
    use metadata::tests::METADATA;
    use std::fs::{copy, create_dir, create_dir_all, write, File};
    use tempfile::{tempdir, TempDir};

    struct Fixture {
        builder: Builder,
        _dir: TempDir,
    }

    impl Fixture {
        fn collect(&self) -> Result<Option<Collection>> {
            Ok(Collection::from(
                &self.builder.config.toml.input,
                &self.builder.config.toml.output,
                &self.builder.config,
            )?)
        }
    }

    fn setup(resize: Option<(u32, u32)>) -> Result<Fixture> {
        let dir = tempdir()?;
        let input = dir.path().join("input");
        let output = dir.path().join("output");
        let theme = dir.path().join("theme");
        let template_dir = theme.join("templates");

        create_dir_all(&input)?;
        create_dir_all(&output)?;
        create_dir_all(&template_dir)?;
        File::create(template_dir.join("index.html"))?;

        let config = config::TomlConfig {
            input: input,
            output: output,
            theme: config::Theme {
                path: theme,
                image_columns: 4,
                collection_columns: 3,
            },
            thumbnail: config::Thumbnail {
                width: 300,
                height: 200,
            },
            resize: resize.and_then(|r| {
                Some(config::Resize {
                    width: r.0,
                    height: r.1,
                })
            }),
        };

        Ok(Fixture {
            builder: Builder::new(Config::from(config)?)?,
            _dir: dir,
        })
    }

    #[test]
    fn empty_dir_is_none_collection() -> Result<()> {
        let f = setup(None)?;
        let collection = f.collect()?;
        assert!(collection.is_none());
        Ok(())
    }

    #[test]
    fn no_image_is_none_collection() -> Result<()> {
        let f = setup(None)?;
        File::create(f.builder.config.toml.input.join("foo.bar"))?;
        let collection = f.collect()?;
        assert!(collection.is_none());
        Ok(())
    }

    #[test]
    fn single_image_is_some_collection() -> Result<()> {
        let f = setup(None)?;
        File::create(f.builder.config.toml.input.join("test.jpg"))?;
        let collection = f.collect()?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert_eq!(collection.items.len(), 1);
        Ok(())
    }

    #[test]
    fn choose_metadata_thumbnail() -> Result<()> {
        let f = setup(None)?;
        File::create(&f.builder.config.toml.input.join("1.jpg"))?;
        File::create(&f.builder.config.toml.input.join("2.jpg"))?;
        File::create(&f.builder.config.toml.input.join("3.jpg"))?;
        write(
            f.builder.config.toml.input.join("index.md"),
            "Thumbnail: 2.jpg",
        )?;

        let collection = f.collect()?.unwrap();
        assert_eq!(
            collection.thumbnail,
            f.builder.config.toml.input.join("2.jpg")
        );
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.builder.config.toml.input.join("test.jpg");
        File::create(&image_path)?;
        write(
            f.builder.config.toml.input.join("index.md"),
            "Thumbnail: doesnotexist.jpg",
        )?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail_on_conflict() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.builder.config.toml.input.join("test.jpg");
        File::create(&image_path)?;
        write(
            f.builder.config.toml.input.join("index.md"),
            "Thumbnail: doesnotexist.jpg",
        )?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_subdir_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let subdir = f.builder.config.toml.input.join("a");
        create_dir(&subdir)?;
        let image_path = subdir.join("test.jpg");
        File::create(&image_path)?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn single_image_in_subdir() -> Result<()> {
        let f = setup(None)?;
        let subdir = f.builder.config.toml.input.join("a");
        create_dir(&subdir)?;
        File::create(subdir.join("test.jpg"))?;

        let collection = f.collect()?;
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

        File::create(f.builder.config.toml.input.join("test.jpg"))?;

        write(f.builder.config.toml.input.join("index.md"), METADATA)?;
        let collection = f.collect()?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert!(collection.metadata.title == "foo");
        Ok(())
    }

    #[test]
    fn process_copy() -> Result<()> {
        let f = setup(None)?;

        // Copy test.jpg, which is 900x600 pixels to the root input dir.
        copy(
            "data/test.jpg",
            f.builder.config.toml.input.join("test.jpg"),
        )?;

        f.builder.build()?;
        let copy_name = f.builder.config.toml.output.join("test.jpg");
        let thumb_name = f.builder.config.toml.output.join("thumbnails/test.jpg");

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
        copy(
            "data/test.jpg",
            f.builder.config.toml.input.join("test.jpg"),
        )?;

        f.builder.build()?;
        let copy_name = f.builder.config.toml.output.join("test.jpg");
        let thumb_name = f.builder.config.toml.output.join("thumbnails/test.jpg");

        assert!(copy_name.exists());
        assert!(thumb_name.exists());

        let copy_dims = image::image_dimensions(copy_name)?;
        assert_eq!(copy_dims, (600, 400));

        let thumb_dims = image::image_dimensions(thumb_name)?;
        assert_eq!(thumb_dims, (300, 200));

        Ok(())
    }
}
