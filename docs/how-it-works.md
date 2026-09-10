---
title: How it works
---

This page walks the build from `rekyll build` to files on disk, one stage at
a time. Each stage names the file that does it, the Jekyll code it copies,
and the one Ruby quirk you'd get wrong if you wrote it from the spec instead
of from Jekyll. If you wanted to write your own, this is the order I'd do it
in, and every stage has a test that tells you when it's right.

The whole thing is about 5,900 lines of Rust across 16 files. The parts that
aren't Jekyll-specific come from crates: `liquid` for templates, `pulldown-cmark`
for Markdown parsing, `grass` for Sass, `yaml-rust2` for YAML events,
`chrono` for time, `strftime-ruby` for date formatting, `tiny_http` for the
preview server.

## The pipeline

```
_config.yml ──► config.rs ──┐
                            ▼
source tree ──► site.rs ──► Site { pages, posts, collections, static files }
                            │
                            ▼
              render.rs ──► for each document, then each page:
                              Liquid → converter → layouts
                            │
                            ▼
              build.rs  ──► clean _site, write everything
```

`src/build.rs` is the 90-line version of that diagram. Start there.

## 1. Configuration

**File:** `src/config.rs`, `src/yaml.rs`. **Jekyll:** `configuration.rb`.

Jekyll starts from a big `DEFAULTS` hash and deep-merges your `_config.yml`
over it. rekyll keeps that hash as a YAML string in `config.rs` so it goes
through the same parser your config does. Then `add_default_collections`
makes sure a `posts` collection exists with `output: true`, and the
top-level `permalink` style becomes the posts collection's permalink
template.

**The quirk:** Ruby's YAML is YAML 1.1, and every Rust YAML crate is 1.2.
`yes` is a boolean, `010` is octal (8), `1_000` is a thousand, `12:00` is
43200 (base 60, really), and `1e3` is a *string* because 1.1's float pattern
insists on a decimal point. `yaml.rs` uses `yaml-rust2` for the parsing but
throws away its type guesses and reruns Psych's own rules
(`scalar_scanner.rb`) over the raw text. Quoted scalars are never resolved.

**Test:** `./tests/harness/units_diff.sh` diffs 50-odd scalars against
Psych.

## 2. Reading the tree

**File:** `src/site.rs`. **Jekyll:** `reader.rb`, `entry_filter.rb`,
`page.rb`, `readers/*.rb`.

`Site::read` walks the source directory. Each entry gets filtered
(`EntryFilter`: leading `.`, `_`, `#` or `~` is hidden unless `include`
lists it; `exclude` hides more), then sorted into directories, pages and
static files. A file is a page if it starts with `---`; that's the whole
test (`Utils.has_yaml_header?`). Everything else is a static file and gets
copied.

`_posts` is hidden by the filter but read on purpose, in every directory,
so `blog/_posts` works. Other collections come from `_<label>` at the root.
Layouts come from `_layouts`, keyed by path without extension, and `_data`
is loaded recursively into `site.data`.

Front matter is split off with Jekyll's exact regex, which matters twice: it
uses `\s`, so CRLF files parse, and the trailing `\s*` swallows the blank
line after the closing `---`, which you can see whenever a template prints
raw `content`.

**The quirk:** pages are sorted by bare filename, not path. That's why the
nav in [Getting started]({{ "/getting-started/" | relative_url }}) lists About before Home.
(And if two pages share a basename, Jekyll's own order is filesystem
dependent; see [Limits]({{ "/limits/" | relative_url }}).)

**Test:** fixtures `01-static`, `02-page-layout`, `10-pages`.

## 3. Dates

**File:** `src/time.rs`, `src/document.rs`. **Jekyll:** `utils.rb`
(`parse_date`), `document.rb`.

A post's date comes from front matter, or failing that from the filename.
Either way Jekyll runs `Time.parse(string).localtime`, with the process
timezone set to your `timezone:`. `time.rs` does the same with `chrono-tz`:
parse, and if there's no zone on it, interpret the wall-clock time in the
site zone (Psych's unzoned-means-UTC rule already happened at stage 1 if the
date came from YAML). DST gaps resolve the way Ruby resolves them, to the
earlier candidate.

`RTime` carries the zone abbreviation alongside the instant, because `%Z`
prints `EST` for a time that came through `localtime` and nothing for one
built from a numeric offset.

**The quirk:** `strftime`. Ruby's has flags chrono's doesn't (`%-d` for no
padding, `%^b` for upcase, `%_H` for space padding, widths like `%10A`), and
Jekyll's permalinks and the `date` filter lean on all of them. The
`strftime-ruby` crate reproduces them; I had a hand-written version and the
crate beat it on three edge cases.

**Test:** `units_diff.sh` diffs 23 formats against Ruby; fixture
`08-timezone` covers `America/New_York` end to end.

## 4. URLs

**File:** `src/url.rs`, `src/document.rs` (`UrlDrop`). **Jekyll:** `url.rb`,
`drops/url_drop.rb`.

A page's URL is `/:path/:basename:output_ext`, or `/:path/` for an index,
with the permalink style deciding whether an `.html` page becomes
`/about/` or `/about.html`. A document's URL is its collection's template
(the `permalink` style for posts, `/:collection/:path` otherwise) with every
`:placeholder` filled in from the `UrlDrop`: `:year`, `:month`, `:title`,
`:categories`, `:slug` and a dozen more. Both then go through
`sanitize_url` (force a leading slash, squeeze doubles, kill `..`).

**The quirk:** placeholder names may end in an underscore, and
`/:month_:day` has to read as `:month` followed by a literal `_`. Jekyll
tries the name with the underscore first and falls back without it.
`generate_url_from_drop` does the same dance.

**Test:** fixture `03-posts`: permalinks, categories, tags.

## 5. The payload

**File:** `src/render.rs` (`Payload`, `site_drop`), `src/lax.rs`.
**Jekyll:** `drops/site_drop.rb`, `drops/document_drop.rb`.

Every template sees `site`, `page`, `layout`, `content`, `paginator` and
`jekyll`. `site` is the config as a fallback hash with the computed parts
layered on: `pages`, `html_pages`, `static_files`, `posts` (newest first),
`documents`, each collection under its own label, `collections` sorted by
label, and `tags`/`categories` as hash-of-arrays in the order the posts
were met.

Jekyll's drops are lazy, so handing one to every render is free. rekyll
builds the site object once, shares it by `Arc`, and remembers where each
document lives inside it so it can patch that object after the document
renders (more on why in stage 6).

**The quirk:** Jekyll runs Liquid with `strict_variables: false`. `{% raw %}{{
page.nope.deeper }}{% endraw %}` is empty, not an error, and real templates
depend on that constantly. The `liquid` crate errors. `lax.rs` wraps the
payload in a value type whose every lookup succeeds and returns nil when
there's nothing there. Hash iteration order also had to match Ruby's
insertion order, which needed a [patch to liquid-core]({{ "/vendor/" | relative_url }}).

## 6. Rendering one thing

**File:** `src/render.rs` (`render_convertible`, `place_in_layouts`).
**Jekyll:** `renderer.rb`.

For every document, then every page, in that order:

1. If the source contains `{% raw %}{{{% endraw %}` or
   `{% raw %}{%{% endraw %}` (and front matter doesn't say
   `render_with_liquid: false`), run it through Liquid with the payload.
2. Run the converter for its extension: Markdown for `.md` and friends, Sass
   for `.scss`/`.sass`, nothing for `.html`.
3. Walk the layout chain: render the layout with `content` set to the output
   so far, then its parent layout, until there's no `layout:` left or a
   layout repeats. `layout` in the payload accumulates the chain's front
   matter as you go up.

**The quirk:** between steps 2 and 3 Jekyll assigns the converted output to
`document.content`. Documents render before pages, so by the time your
index page loops over `site.posts`, `post.content` is HTML, not Markdown.
That's why the payload gets patched after each document: a page rendered
later has to see the converted form. Excerpts (`excerpt.rb`) are the same
pipeline minus the layouts, run on the text before `excerpt_separator`.

**Test:** fixtures `07-blog` and `10-pages`.

## 7. The converters

**File:** `src/markdown.rs`, `src/sass.rs`. **Jekyll:** `converters/markdown.rb`
via kramdown 2.4 with `kramdown-parser-gfm`; `jekyll-sass-converter` via
sassc.

Markdown is where most of the bytes are. `pulldown-cmark` does the parsing;
the emitter in `markdown.rs` walks its events with source offsets and writes
HTML the way kramdown does, not the way CommonMark's reference renderer
does. That means two-space indentation per nesting level, a newline between
blocks wherever your source had a blank line, XHTML void elements
(`<br />`), GFM header ids, Rouge's `<div class="language-x highlighter-rouge">`
wrapper on fenced code, `language-plaintext` on inline code, HTML entities
turned into real characters, and smart quotes whose direction depends on the
character before them in the source.

Sass goes through `grass` in expanded mode and then gets reformatted into
libsass's `:compact` style (one rule per line, a blank line between
top-level blocks, none after a comment).

**The quirk:** there are a dozen in `markdown.rs`, each with a comment
naming it. The one that cost the most: kramdown decides whether `'` opens
or closes a quote by looking at the preceding character in the *source*, so
a quote right after inline code sees the closing backtick. The most recent:
a code span keeps its newlines, and a lone backtick between spaces isn't a
code span at all. I found that one writing this page, which is a decent
argument for writing docs.

**Test:** `./tests/harness/md_diff.sh` diffs a corpus against kramdown;
fixture `05-sass` covers Sass.

## 8. Filters and tags

**File:** `src/filters.rs`, `src/tags.rs`. **Jekyll:** `filters.rb`,
`filters/*.rb`, `tags/*.rb`.

Jekyll's own filters (`slugify`, `date_to_xmlschema`, `where_exp`, `jsonify`,
`relative_url` and the rest) are each a plain function registered by name.
The tags (`include`, `include_relative`, `link`, `post_url`, `highlight`)
parse their own arguments, because none of them follow Liquid's grammar.

**The quirk:** ten standard Liquid filters had to be overridden because
Ruby's versions differ. `escape` writes `&#39;` for an apostrophe where
liquid-rust writes `&#x27;`. `url_encode` turns a space into `+`.
`capitalize` downcases the rest of the word. `split` drops trailing empty
strings. `3 | divided_by: 2` is `1` because Ruby integer division stays
integer, and `1.5 | plus: 1.5` is `3.0` because `Float#to_s` keeps the point.

**Test:** `./tests/harness/filters_diff.sh` diffs 110 expressions against
Ruby Liquid.

## 9. Writing

**File:** `src/build.rs`. **Jekyll:** `site.rb` (`process`), `cleaner.rb`.

Everything is rendered in memory first, so a template error leaves your
`_site` untouched. Then the cleaner deletes everything in `_site` except
`keep_files` (`.git` and `.svn` by default), rendered pages and documents
are written to their destinations (a URL ending in `/` gets `index.html`),
and static files are copied.

**Test:** every fixture; `diff.sh` compares whole `_site` trees.

## 10. Serving

**File:** `src/serve.rs`.

`tiny_http` answers requests, `mime_guess` picks content types, and the
rest maps a URL onto `_site`: a directory serves its `index.html`, `/about`
serves `about.html`, and anything with `..` in it is refused. A thread polls
the source tree twice a second and rebuilds when a file count or newest
mtime changes; each HTML response gets a small script appended that polls
`/__rekyll_live` and reloads when the build counter moves. The script is
added to the response only, never to the file, so `_site` still diffs clean
against Jekyll.

**Test:** `./tests/harness/serve_smoke.sh`.

## Where to start if you're replicating this

Stages 1, 2 and 9 get you a site generator that copies files and expands
templates. Then do 6 with `.html` only, then 3 and 4 so posts have URLs,
then 7 for Markdown. Keep the differential harness running from the first
day; you can't tell by reading the output whether it matches, and Jekyll's
behaviour is the spec. The [Testing]({{ "/testing/" | relative_url }}) page is how to set that up.
