---
title: Features
---

Everything here is checked byte-for-byte against Jekyll 4.3.2. See
[Testing](/testing/) for how.

## Configuration

rekyll reads `_config.yml` or `_config.yaml` and applies the same `DEFAULTS`
Jekyll does, merged the same way. Your scalars get resolved under Ruby's YAML
1.1 rules, not 1.2, which is a bigger deal than it sounds: `yes` and `on` are
booleans, `010` is octal, `1_000` and `1,000` both come out as 1000, and `1e3`
is a string, because the float pattern wants a decimal point in it. Anything
you quote is left alone as a string.

Keys you can use include `collections`, `defaults`, `include`, `exclude`,
`keep_files`, `permalink`, `timezone`, `time`, `future`, `unpublished`,
`excerpt_separator`, `markdown_ext`, `baseurl` and `url`.

## Content

Pages work with or without front matter, and posts come from `_posts` in any
directory, so `blog/_posts` is fine too. Custom collections get their own
`output` and `permalink` settings. Static files are copied across, and if one
lives inside a collection it goes wherever that collection's URL template puts
it. Your `_data` folder is read as `.yml` and `.yaml`, nested by subdirectory.
CRLF and LF line endings both work.

## Permalinks

All five built-in styles work: `none`, `date`, `ordinal`, `pretty` and
`weekdate`. If you write your own template you get every `UrlDrop` placeholder,
including
`:year`, `:month`, `:day`, `:i_month`, `:short_month`, `:y_day`, `:week`,
`:short_day`, `:categories`, `:slug`, `:title`, `:name`, `:collection`, `:path`
and `:output_ext`. A `permalink` in front matter beats the template, and any
directories between the collection root and the file turn into categories.

## Dates

Dates come from the filename or from front matter, and either way they resolve
the way `Time.parse(...).localtime` does, in your site's timezone. Zoned,
unzoned and date-only values all work, and so do daylight saving gaps. The
`future` and `published` settings decide what actually gets written.

Worth knowing, because it surprised me: if your timezone is
`America/New_York` and you write `date: 2020-01-02 03:04:05` with no zone on
it, the post publishes at `/2020/01/01/`. Ruby's YAML parser reads an unzoned
time as a UTC instant, and `localtime` then drags it back to the previous
evening. rekyll does the same thing, so your posts land where Jekyll would put
them.

## Layouts And Includes

Layout chains work, along with nested layout data merging and `layout: none`.
The `include` tag takes quoted, unquoted and variable parameters, and
`include_relative` resolves against the directory of whatever file is doing the
including. The `link` and `post_url` tags turn a source path into its output
URL with your baseurl on the front. The `highlight` tag works with and without
`linenos`. Front-matter `defaults` follow Jekyll's precedence, where a longer
scope path wins and a typed scope breaks a tie.

## Filters

Jekyll's own filters:

`slugify`, `xml_escape`, `cgi_escape`, `uri_escape`, `number_of_words`,
`array_to_sentence_string`, `jsonify`, `to_integer`, `inspect`,
`normalize_whitespace`, `markdownify`, `smartify`, `date_to_string`,
`date_to_long_string`, `date_to_xmlschema`, `date_to_rfc822`, `relative_url`,
`absolute_url`, `strip_index`, `push`, `pop`, `shift`, `unshift`, `where`,
`group_by`, `group_by_exp`, `find`, `find_exp`, `where_exp`, `sample`,
`sassify`, `scssify`

The three `_exp` filters take an expression and evaluate it once per item with
your variable bound to that item, the same as Jekyll, so
`{% raw %}{{ site.posts | where_exp: "p", "p.tags contains 'rust'" }}{% endraw %}`
does what you'd expect. You can use filters inside the expression too.

Standard Liquid filters that had to be overridden because Ruby does something
different:

`date`, `escape`, `escape_once`, `url_encode`, `url_decode`, `capitalize`,
`split`, `divided_by`, `sort`, `map`

Ruby's `escape` gives you `&#39;` where liquid-rust gives `&#x27;`,
`url_encode` turns a space into `+`, `capitalize` downcases everything after
the first letter, `split` throws away trailing empty fields, and dividing two
integers stays an integer, so `3 | divided_by: 2` is 1. Floats print Ruby-style
too, which is why `1.5 | plus: 1.5` is `3.0` and not `3`. Everything else in
the Liquid standard library comes from the `liquid` crate as-is.

## Markdown

Markdown goes through an emitter written to match kramdown 2.4 with
`kramdown-parser-gfm` under Jekyll's options. Blocks get indented two spaces
per nesting level, and blank lines in your source come out as newlines between
blocks. Kramdown does both of those and no Rust Markdown crate does, which is
why the emitter exists. Void elements are XHTML, so raw HTML gets rewritten to
`<img ... />`. Headers get GFM ids, with `-1` and `-2` on the end if you repeat
one. Code blocks get Rouge's wrapper, and inline code carries the
`language-plaintext` class that Jekyll's `default_lang` puts there.

Entities turn into real characters across the whole HTML4 named set, so
`&rarr;` and `&frac12;` work and not just the common ones. Typography covers
smart quotes, dashes, ellipses and guillemets, down to the non-breaking space
kramdown sticks next to a guillemet. Tables with alignment, footnotes and
strikethrough all work.

## Sass

Both `.scss` and `.sass` build, with front matter, using `_sass` as the load
path plus whatever you put in `sass.load_paths`. You get libsass `:compact`
output by default, since that's what jekyll-sass-converter produces under
sassc. Set `sass.style` to `expanded` or `compressed` if you'd rather.

## Excerpts

An excerpt is everything up to `excerpt_separator`, which you can set per post
or site-wide. Any Markdown link reference definitions the excerpt points at get
appended to it, so a `[text][ref]` still resolves once the rest of the post has
been cut off.
