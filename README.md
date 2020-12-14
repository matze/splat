## splat

A simple [sigal](https://github.com/saimn/sigal) clone written in Rust.

<a href="https://matze.github.io/splat/"><img alt="Example output" src="https://i.imgur.com/EHyhrRw.jpg"/></a>

### Usage

splat is a command line application and uses various sub commands. First, create
a new base configuration using `splat new` and edit `.splat.toml` to your liking,
especially adapt the `input` and `output` paths. `output` will be created if it
does not exist. Then run `splat build` to generate the static output.
