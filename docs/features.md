---
title: Features
---

Everything on this page is verified byte-identical against Jekyll 4.3.2. See
[Testing](/testing/) for how.

## Configuration

- Reads `_config.yml` and `_config.yaml`.
- Applies Jekyll's full `DEFAULTS` set and deep-merge semantics.
- Resolves scalars with Ruby's YAML 1.1 rules. `yes`/`on` are booleans, `010` is octal, `1_000` and `1,000` are 1000, `1e3` is a string.
- Never type-resolves quoted scalars.
- Supports `collections`, `defaults`, `include`, `exclude`, `keep_files`, `permalink`, `timezone`, `time`, `future`, `unpublished`, `excerpt_separator`, `markdown_ext`, `baseurl`, `url`.

## Content

- Pages with and without front matter.
- Posts from `_posts` in any directory, including nested ones like `blog/_posts`.
- Custom collections, with `output` and `permalink` per collection.
- Static files, including files inside a collection.
- `_data` directory as `.yml` and `.yaml`, nested by subdirectory.
- CRLF and LF sources.

## Permalinks

- All five built-in styles: `none`, `date`, `ordinal`, `pretty`, `weekdate`.
- Custom templates with every `UrlDrop` placeholder, including `:year`, `:month`, `:day`, `:i_month`, `:short_month`, `:y_day`, `:week`, `:short_day`, `:categories`, `:slug`, `:title`, `:name`, `:collection`, `:path`, `:output_ext`.
- Per-document `permalink` in front matter.
- Categories taken from directory path.

## Dates

- Filename dates and front-matter dates.
- Resolves in the site timezone the way `Time.parse(...).localtime` does.
- Handles zoned, unzoned and date-only values, plus DST gaps.
- `future` and `published` filtering.

## Layouts and includes

- Layout chains, nested layout data merging, `layout: none`.
- `{% raw %}{% include %}{% endraw %}` with quoted, unquoted and variable parameters.
- `{% raw %}{% include_relative %}{% endraw %}`, resolved against the including file's directory.
- `{% raw %}{% link %}{% endraw %}` and `{% raw %}{% post_url %}{% endraw %}`.
- `{% raw %}{% highlight %}{% endraw %}`, with and without `linenos`.
- Front-matter `defaults` with Jekyll's scope precedence.

## Liquid

- Undefined variables render empty, matching `strict_variables: false`.
- Unknown filters pass their input through, matching `strict_filters: false`.
- Hash iteration order matches Ruby's insertion order.
- Floats render Ruby-style, so `1.5 | plus: 1.5` gives `3.0`.

## Filters

Jekyll's own:

`slugify`, `xml_escape`, `cgi_escape`, `uri_escape`, `number_of_words`,
`array_to_sentence_string`, `jsonify`, `to_integer`, `inspect`,
`normalize_whitespace`, `markdownify`, `smartify`, `date_to_string`,
`date_to_long_string`, `date_to_xmlschema`, `date_to_rfc822`, `relative_url`,
`absolute_url`, `strip_index`, `push`, `pop`, `shift`, `unshift`, `where`,
`group_by`, `find`

Standard Liquid filters overridden to match Ruby's behaviour:

`date`, `escape`, `escape_once`, `url_encode`, `url_decode`, `capitalize`,
`split`, `divided_by`, `sort`, `map`

The rest of the Liquid standard library comes from the `liquid` crate.

## Markdown

Matches kramdown 2.4 with `kramdown-parser-gfm` under Jekyll's options.

- Two-space indentation per block nesting level.
- Source blank lines preserved as newlines between blocks.
- XHTML void elements, including raw HTML rewritten to `<img ... />`.
- GFM header ids, with `-1`/`-2` suffixes for repeats.
- Rouge's code wrapper, and the `language-plaintext` class on inline code.
- `entity_output: as_char` across the full HTML4 named entity set.
- Smart quotes, dashes, ellipses and guillemets, including the non-breaking space kramdown binds to guillemets.
- Tables with alignment, footnotes, strikethrough.

## Sass

- `.scss` and `.sass` sources with front matter.
- `_sass` load path, plus `sass.load_paths`.
- libsass `:compact` output, which is jekyll-sass-converter's default. `expanded` and `compressed` also work.

## Excerpts

- Content up to `excerpt_separator`, per-document or site-wide.
- Appends the link reference definitions the excerpt refers to, so `[text][ref]` still resolves.
