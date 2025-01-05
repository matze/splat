# 📷 splat

![Rust](https://github.com/matze/splat/workflows/Rust/badge.svg)

**splat** is a command line application to generate static photo galleries from
a source directory of image files. It is a spiritual cousin of
[sigal](https://github.com/saimn/sigal) but written in Rust.

<a href="https://matze.github.io/splat/"><img alt="Example output" src="https://github.com/matze/splat/blob/master/example/screenshot.jpg"/></a>

<p align="center"><strong><a href="https://matze.github.io/splat/">DEMO OUTPUT</a></strong></p>


## Features

- Generates static galleries for no-nonsense hosting.
- Easy installation and simple usage.
- Clean and beautiful Tailwind CSS based builtin theme.


## Usage

**splat** is a command line application and besides a source directory of image
files it requires a `splat.toml` configuration file and a theme file containing
an HTML template as well as optional assets.

To create an example configuration run `splat new` and edit `splat.toml` to your
liking, especially adapt the `input` and `output` paths. `output` will be
created if it does not exist. Then run `splat build` to generate the static
output.

> [!IMPORTANT]
> The example theme relies on the Tailwind CSS v4.0 beta compiler. Make sure to
> install it if you want to use the theme.


## Templates

Templates must be written in [tera
syntax](https://keats.github.io/tera/docs/#templates). The following hierarchy
of variables is available:

- `collection`
  - `title` of this collection
  - `breadcrumbs`
    - `path` to the corresponding page
    - `title` of the corresponding page
- `children` of sub-collections
  - `path` to the collection
  - `title` of the collection
  - `thumbnail` of the collection
- `images` for this collection
  - `path` to the image
  - `width` and `height` of the image
  - `thumbnail` of the image


## License

[MIT](./LICENSE)
