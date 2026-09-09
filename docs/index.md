---
title: rekyll
---

rekyll builds Jekyll sites. You point it at the same folder Jekyll uses, with
the same `_config.yml`, `_posts`, `_layouts`, `_includes` and `_data`, and it
writes a `_site` that matches what Jekyll 4.3.2 would have written, byte for
byte. It doesn't run plugins, so it's output-compatible rather than
plugin-compatible.

## Overview

A build reads your site, works out the front matter and dates, renders Liquid,
converts Markdown and Sass, and writes everything out. Your config gets parsed
with Ruby's YAML 1.1 rules (the ones Ruby's parser actually uses, not the 1.2
rules every Rust YAML crate implements), so `yes` is a boolean and `010` is
octal. Liquid runs lax the way Jekyll runs it, so an undefined variable renders
empty instead of blowing up, and a filter you don't have just passes its input
through. Markdown matches kramdown 2.4 in GFM mode, and Sass comes out in
libsass `:compact` style, which is what jekyll-sass-converter gives you by
default.

It's one binary, about 8.6 MB. No Ruby, no gems, nothing to install next to it.

## Requirements

Rust 1.94 or newer to build it, and nothing at all to run it. You only need
Jekyll 4.3.2 around if you want to run the differential tests.

## Install

```
cargo install --path . --locked
```

Don't drop the `--locked`. Without it `cargo install` re-resolves everything
and grabs a version of kstring that wants a newer rustc than this crate builds
against.

## Usage

```
rekyll build -s path/to/site -d path/to/_site
```

Both flags are optional. `-s` defaults to wherever you are and `-d` to
`<source>/_site`.

If you want to look at the result, build and serve in one go:

```
rekyll serve -s docs -d docs/_site
```

That puts it on `http://127.0.0.1:4000`, same default as Jekyll, and `-H` and
`-P` change the host and port. Add `--skip-initial-build` if you already have a
`_site` and just want to serve it. It's a preview server, so it won't watch for
changes and you shouldn't point it at a network. If you don't want it at all,
build with `--no-default-features` and you get a build-only binary.

## Speed

Same site, same machine, and the output is identical in both columns.

| posts | Jekyll 4.3.2 | rekyll |
|-------|--------------|--------|
| 302   | 0.58s        | 0.17s  |
| 1202  | 1.43s        | 0.66s  |
