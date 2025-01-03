## splat

![Rust](https://github.com/matze/splat/workflows/Rust/badge.svg)

A simple [sigal](https://github.com/saimn/sigal) clone written in Rust.

<a href="https://matze.github.io/splat/"><img alt="Example output" src="https://github.com/matze/splat/blob/master/example/screenshot.jpg"/></a>

<p align="center"><strong><a href="https://matze.github.io/splat/">DEMO OUTPUT</a></strong></p>

### Usage

splat is a command line application to generate static photo galleries from a
source directory of image files, a `splat.toml` configuration file and a theme
file containing an HTML template and optional assets.

To create an example configuration run `splat new` and edit `splat.toml` to your
liking, especially adapt the `input` and `output` paths. `output` will be
created if it does not exist. Then run `splat build` to generate the static
output.

> [!IMPORTANT]
> The example theme relies on the Tailwind CSS v4.0 beta compiler. Make sure to
> install it if you want to use the theme.
