mod config;
mod metadata;
mod process;

use anyhow::{anyhow, Result};
use clap::Parser;
use config::Config;
use metadata::Metadata;
use process::{copy_recursively, is_older, process, Process};
use rayon::prelude::*;
use serde::Serialize;
use std::fs::{create_dir_all, read_dir, write};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::LazyLock;
use std::thread;

#[derive(Parser)]
#[clap(name = "splat", about = "Static photo gallery generator")]
enum Commands {
    #[clap(about = "Build static gallery", visible_alias = "b")]
    Build,

    #[clap(
        about = "Create new splat.toml config and example theme",
        visible_alias = "n"
    )]
    New,
}

/// Image item to process.
pub struct Item {
    /// Source image.
    from: PathBuf,
    /// Target image.
    to: PathBuf,
    /// Thumbnail generated from `from`.
    thumbnail: PathBuf,
}

/// A [`Collection`] contains either other [`Collection`]s or a bunch of [`Item`]s.
struct Collection {
    path: PathBuf,
    /// Child collections.
    collections: Vec<Collection>,
    /// Image items.
    items: Vec<Item>,
    /// Parsed metadata from index.md's.
    metadata: Metadata,
    /// Path to the process thumbnail.
    thumbnail: PathBuf,
}

/// A fullsize image, its thumbnail and its image dimensions as used in the HTML templates.
#[derive(Clone, Serialize)]
struct Image<'a> {
    /// Path to the image.
    path: &'a str,
    /// Path to the thumbnail.
    thumbnail: PathBuf,
    /// Width of the image.
    width: u32,
    /// Height of the image.
    height: u32,
}

/// Individual subcollection.
#[derive(Clone, Serialize)]
struct Child<'a> {
    /// Path to the collection.
    path: String,
    /// Collection thumbnail.
    thumbnail: PathBuf,
    /// Title of the collection.
    title: &'a str,
}

/// A breadcrumb link.
#[derive(Serialize)]
struct Link<'a> {
    title: &'a str,
    path: String,
}

/// Context passed to the tera template.
#[derive(Serialize)]
struct Output<'a> {
    /// Title of the collection.
    title: &'a str,
    /// Description of the collection.
    description: &'a str,
    /// Breadcrumb links leading to this collection.
    breadcrumbs: Vec<Link<'a>>,
    /// Subcollections.
    children: Vec<Child<'a>>,
    /// Images part of this collection.
    images: Vec<Image<'a>>,
}

/// Spinner images.
static SPINNERS: LazyLock<[&str; 4]> = LazyLock::new(|| ["⠖", "⠲", "⠴", "⠦"]);

/// Compute breadcrumb links from a list of strings.
fn breadcrumbs_to_links(breadcrumbs: &[String]) -> Vec<Link> {
    let mut path = String::from(".");

    let mut links: Vec<_> = breadcrumbs
        .iter()
        .rev()
        .map(|breadcrumb| {
            let link = Link {
                title: breadcrumb,
                path: path.clone(),
            };
            path = format!("{}/..", path);
            link
        })
        .collect();

    links.reverse();
    links
}

fn output_path_to_root(output: &Path) -> PathBuf {
    let mut path = PathBuf::new();

    for _ in 1..output.iter().count() {
        path = path.join("..");
    }

    path
}

impl<'a> Image<'a> {
    fn new(item: &'a Item) -> Result<Self> {
        let (width, height) = image::image_dimensions(&item.to)?;

        let path = item
            .to
            .file_name()
            .ok_or_else(|| anyhow!("{:?} is not a file", item.to))?
            .to_str()
            .ok_or_else(|| anyhow!("Failed to stringify {:?}", item.to))?;

        let thumbnail = PathBuf::from("thumbnails").join(
            item.thumbnail
                .file_name()
                .ok_or_else(|| anyhow!("{:?} has no file name", item.thumbnail))?,
        );

        Ok(Self {
            thumbnail,
            path,
            width,
            height,
        })
    }
}

impl Item {
    fn new(path: PathBuf, config: &Config) -> Result<Self> {
        let to = config
            .toml
            .output
            .join(path.strip_prefix(&config.toml.input)?);

        Ok(Self {
            thumbnail: to
                .parent()
                .ok_or_else(|| anyhow!("No parent"))?
                .join("thumbnails")
                .join(path.file_name().ok_or_else(|| anyhow!("Path ends in .."))?),
            to,
            from: path,
        })
    }

    fn needs_update(&self) -> bool {
        !self.to.exists()
            || is_older(&self.to, &self.from).unwrap_or_default()
            || !self.thumbnail.exists()
    }

    fn thumbnail_outdated(&self) -> Result<bool> {
        is_older(&self.thumbnail, &self.from)
    }
}

impl<'a> Child<'a> {
    fn from(collection: &'a Collection) -> Result<Self> {
        let path = collection
            .path
            .parent()
            .ok_or_else(|| anyhow!("{:?} has no parent", collection.path))?;

        let filename = collection
            .thumbnail
            .file_name()
            .ok_or_else(|| anyhow!("{:?} has no filename", collection.thumbnail))?;

        let thumbnail = collection
            .thumbnail
            .strip_prefix(path)?
            .parent()
            .ok_or_else(|| anyhow!("{:?} has no parent", collection.thumbnail))?
            .join("thumbnails")
            .join(filename);

        let subdir = collection
            .path
            .file_name()
            .ok_or_else(|| anyhow!("{:?} has no filename", collection.path))?
            .to_string_lossy()
            .to_string();

        Ok(Self {
            thumbnail,
            path: subdir,
            title: &collection.metadata.title,
        })
    }
}

impl Collection {
    fn new(current: &Path, config: &Config) -> Result<Option<Self>> {
        let collections: Vec<Collection> = read_dir(current)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .map(|entry| Collection::new(&entry.path(), config))
            .filter_map(Result::ok)
            .flatten()
            .collect();

        let items: Vec<Item> = read_dir(current)?
            .filter_map(Result::ok)
            .filter(|e| {
                e.path().is_file()
                    && e.path().extension().is_some_and(|ext| {
                        ext == "JPG" || ext == "jpg" || ext == "JPEG" || ext == "jpeg"
                    })
            })
            .map(|e| Item::new(e.path(), config))
            .collect::<Result<Vec<_>>>()?;

        if items.is_empty() && collections.is_empty() {
            return Ok(None);
        }

        let metadata = Metadata::from_path(current)?;

        // Determine thumbnail for this collection. We prioritize the one specified in the metadata
        // over the first item in this collection over the thumbnail of the first child collection.
        let thumbnail = metadata
            .thumbnail
            .as_ref()
            .cloned()
            .or_else(|| {
                items
                    .first()
                    .map_or(collections.first().map(|c| c.thumbnail.clone()), |item| {
                        Some(item.from.clone())
                    })
            })
            .ok_or_else(|| anyhow!("No thumbnail path"))?;

        Ok(Some(Collection {
            path: current.to_owned(),
            collections,
            items,
            metadata,
            thumbnail,
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

/// Build the gallery and all required assets.
fn build(config: &Config) -> Result<()> {
    if !config.toml.input.exists() {
        return Err(anyhow!("{:?} does not exist", config.toml.input));
    }

    if !config.toml.output.exists() {
        create_dir_all(&config.toml.output)?;
    }

    if let Some(static_path) = config.static_path.as_ref() {
        print!("  Copying static data ...");
        copy_recursively(static_path, &config.toml.output)?;
        println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Copied static data");
    }

    if let Some(processes) = &config.toml.theme.process {
        for process in processes {
            process.run()?;
        }
    }

    let collection =
        Collection::new(&config.toml.input, config)?.ok_or_else(|| anyhow!("No images found"))?;

    let items = collection
        .items()
        .into_iter()
        .filter(|item| item.needs_update())
        .collect::<Vec<_>>();

    let num_items = items.len();
    let (sender, receiver) = mpsc::channel::<Result<()>>();

    let processes = items
        .into_iter()
        .map(|item| Process {
            config,
            item,
            sender: sender.clone(),
        })
        .collect::<Vec<_>>();

    thread::spawn(move || display_progress(num_items, receiver));

    processes.into_par_iter().for_each(|p| {
        if let Err(err) = process(&p) {
            eprintln!("failed to process an image: {err:?}");
        }
    });

    print!("  Writing HTML pages ...");
    // TODO: make "home" configurable
    let mut breadcrumbs: Vec<String> = vec![String::from("home")];
    write_html(config, &collection, &mut breadcrumbs, &config.toml.output)?;
    println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Wrote HTML pages");

    Ok(())
}

fn display_progress(num_items: usize, receiver: mpsc::Receiver<Result<()>>) {
    let num_spinners = SPINNERS.len();

    for i in 0..num_items {
        print!(
            "\x1B[2K\r\x1B[0;36m{}\x1B[0;m Processing {} images ...",
            SPINNERS[i % num_spinners],
            num_items - i
        );

        if let Err(err) = io::stdout().flush() {
            eprintln!("failed to flush stdout: {err:?}");
        }

        let Ok(result) = receiver.recv() else {
            eprintln!("failed to receive item");
            continue;
        };

        if let Err(result) = result {
            println!("\x1B[2K\r\x1B[0;31mE\x1B[0;m {}", result);
        }
    }

    println!(
        "\x1B[2K\r\x1B[0;32m✔\x1B[0;m Processed {} images",
        num_items
    );
}

/// Write out HTML for the given `collection` and `breadcrumbs` into `output`.
fn write_html(
    config: &Config,
    collection: &Collection,
    breadcrumbs: &mut Vec<String>,
    output: &Path,
) -> Result<()> {
    if !output.exists() {
        create_dir_all(output)?;
    }

    for child in &collection.collections {
        let subdir = child
            .path
            .file_name()
            .ok_or_else(|| anyhow!("Path ends in .."))?;

        let output = output.join(subdir);

        breadcrumbs.push(subdir.to_string_lossy().to_string());
        write_html(config, child, breadcrumbs, &output)?;
        breadcrumbs.remove(breadcrumbs.len() - 1);
    }

    let mut images = collection
        .items
        .iter()
        .map(Image::new)
        .collect::<Result<Vec<_>, _>>()?;

    images.sort_by(|a, b| a.thumbnail.cmp(&b.thumbnail));

    let mut children = collection
        .collections
        .iter()
        .map(Child::from)
        .collect::<Result<Vec<_>, _>>()?;

    children.sort_by(|a, b| b.title.cmp(a.title));

    let mut context = tera::Context::new();
    let breadcrumbs = breadcrumbs_to_links(breadcrumbs);

    context.insert(
        "collection",
        &Output {
            title: &collection.metadata.title,
            description: &collection.metadata.description,
            breadcrumbs,
            children,
            images,
        },
    );

    let static_path = output_path_to_root(output).join("static");
    context.insert("theme_url", &static_path);

    let index_html = output.join("index.html");

    Ok(write(
        index_html,
        config.templates.render("index.html", &context)?,
    )?)
}

fn run_build() -> Result<()> {
    build(&Config::read()?)
}

/// Write out configuration and default theme.
fn run_new() -> Result<()> {
    let paths = ["theme/static/css", "theme/static/js", "theme/templates"];

    for path in paths {
        let path = PathBuf::from(path);

        if !path.exists() {
            create_dir_all(path)?;
        }
    }

    write(config::TOML_FILENAME, include_str!("../example/splat.toml"))?;

    write(
        "theme/input.css",
        include_str!("../example/theme/input.css"),
    )?;

    write(
        "theme/static/css/photoswipe.css",
        include_str!("../example/theme/static/css/photoswipe.css"),
    )?;

    write(
        "theme/static/js/photoswipe-lightbox.esm.min.js",
        include_str!("../example/theme/static/js/photoswipe-lightbox.esm.min.js"),
    )?;

    write(
        "theme/static/js/photoswipe.esm.min.js",
        include_str!("../example/theme/static/js/photoswipe.esm.min.js"),
    )?;

    println!("\x1B[2K\r\x1B[0;32m✔\x1B[0;m Wrote splat.toml and theme directory");

    Ok(())
}

fn main() {
    let commands = Commands::parse();

    let result = match commands {
        Commands::Build => run_build(),
        Commands::New => run_new(),
    };

    if let Err(err) = result {
        println!("\x1B[2K\r\x1B[0;31mE\x1B[0;m {}", err);
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
        config: Config,
        _dir: TempDir,
    }

    impl Fixture {
        fn collect(&self) -> Result<Option<Collection>> {
            Ok(Collection::new(&self.config.toml.input, &self.config)?)
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

        let config = config::Toml {
            input,
            output,
            theme: config::Theme {
                path: theme,
                process: None,
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
            config: config.try_into()?,
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
        File::create(f.config.toml.input.join("foo.bar"))?;
        let collection = f.collect()?;
        assert!(collection.is_none());
        Ok(())
    }

    #[test]
    fn single_image_is_some_collection() -> Result<()> {
        let f = setup(None)?;
        File::create(f.config.toml.input.join("test.jpg"))?;
        let collection = f.collect()?;
        assert!(collection.is_some());

        let collection = collection.unwrap();
        assert_eq!(collection.items.len(), 1);
        Ok(())
    }

    #[test]
    fn choose_metadata_thumbnail() -> Result<()> {
        let f = setup(None)?;
        File::create(&f.config.toml.input.join("1.jpg"))?;
        File::create(&f.config.toml.input.join("2.jpg"))?;
        File::create(&f.config.toml.input.join("3.jpg"))?;
        write(f.config.toml.input.join("index.md"), "Thumbnail: 2.jpg")?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, f.config.toml.input.join("2.jpg"));
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.config.toml.input.join("test.jpg");
        File::create(&image_path)?;
        write(
            f.config.toml.input.join("index.md"),
            "Thumbnail: doesnotexist.jpg",
        )?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_root_thumbnail_on_conflict() -> Result<()> {
        let f = setup(None)?;
        let image_path = f.config.toml.input.join("test.jpg");
        File::create(&image_path)?;
        write(
            f.config.toml.input.join("index.md"),
            "Thumbnail: doesnotexist.jpg",
        )?;

        let collection = f.collect()?.unwrap();
        assert_eq!(collection.thumbnail, image_path);
        Ok(())
    }

    #[test]
    fn choose_subdir_thumbnail() -> Result<()> {
        let f = setup(None)?;
        let subdir = f.config.toml.input.join("a");
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
        let subdir = f.config.toml.input.join("a");
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

        File::create(f.config.toml.input.join("test.jpg"))?;

        write(f.config.toml.input.join("index.md"), METADATA)?;
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
        copy("data/test.jpg", f.config.toml.input.join("test.jpg"))?;

        build(&f.config)?;
        let copy_name = f.config.toml.output.join("test.jpg");
        let thumb_name = f.config.toml.output.join("thumbnails/test.jpg");

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
        copy("data/test.jpg", f.config.toml.input.join("test.jpg"))?;

        build(&f.config)?;
        let copy_name = f.config.toml.output.join("test.jpg");
        let thumb_name = f.config.toml.output.join("thumbnails/test.jpg");

        assert!(copy_name.exists());
        assert!(thumb_name.exists());

        let copy_dims = image::image_dimensions(copy_name)?;
        assert_eq!(copy_dims, (600, 400));

        let thumb_dims = image::image_dimensions(thumb_name)?;
        assert_eq!(thumb_dims, (300, 200));

        Ok(())
    }

    #[test]
    fn breadcrumb_links() -> Result<()> {
        let breadcrumbs = [
            String::from("foo"),
            String::from("bar"),
            String::from("baz"),
        ];
        let links = breadcrumbs_to_links(&breadcrumbs);
        assert_eq!(links[0].title, "foo");
        assert_eq!(links[0].path, "./../..");
        assert_eq!(links[1].title, "bar");
        assert_eq!(links[1].path, "./..");
        assert_eq!(links[2].title, "baz");
        assert_eq!(links[2].path, ".");
        Ok(())
    }
}
