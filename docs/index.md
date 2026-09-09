---
title: rekyll
---

Jekyll, but make it Rust. A static site generator that reads a Jekyll site and
writes the same `_site` Jekyll 4.3.2 would, byte for byte.

It is not plugin-compatible. It is output-compatible.

## What it does

- Builds any Jekyll site that uses core Jekyll features, with byte-identical output.
- Reads `_config.yml` with Ruby's YAML 1.1 rules, so `yes` is a boolean and `010` is octal.
- Handles pages, posts, custom collections, layouts, includes and `_data`.
- Renders Liquid, including Jekyll's filters and the `include`, `include_relative`, `link`, `post_url` and `highlight` tags.
- Converts Markdown to match kramdown 2.4 in GFM mode.
- Compiles Sass and SCSS in libsass `:compact` style.
- Resolves dates in the site timezone, including DST and unzoned front matter.
- Serves the built site with `rekyll serve`, for previewing.
- Ships as one self-contained binary. No Ruby, no gems.

## Requirements

- Rust 1.94 or newer to build.
- Nothing at runtime.
- Jekyll 4.3.2 only if you want to run the differential tests.

## Install

```
cargo install --path . --locked
```

Use `--locked`. Without it `cargo install` re-resolves dependencies and picks a
`kstring` that needs a newer rustc.

## Usage

```
rekyll build -s path/to/site -d path/to/_site
```

Both flags are optional. `-s` defaults to the current directory, `-d` to
`<source>/_site`.

To build and preview in one step:

```
rekyll serve -s docs -d docs/_site
```

Defaults to `http://127.0.0.1:4000`, the same as Jekyll. Override with `-H`
and `-P`. Pass `--skip-initial-build` to serve an existing `_site` without
rebuilding.

The server is a preview server. It does not watch or rebuild, and it is not
meant to face a network. Build it out with `--no-default-features` if you want
a build-only binary.

## Speed

Same machine, same generated site, identical output at both sizes.

| posts | Jekyll 4.3.2 | rekyll |
|-------|--------------|--------|
| 302   | 0.58s        | 0.17s  |
| 1202  | 1.43s        | 0.66s  |
