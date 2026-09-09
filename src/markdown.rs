//! Kramdown-compatible Markdown conversion.
//!
//! Jekyll renders Markdown with kramdown 2.4 in GFM mode. No Rust crate
//! reproduces kramdown's HTML, and the differences are not cosmetic: kramdown
//! indents nested block elements two spaces per level, preserves source blank
//! lines as newlines between blocks, emits XHTML-style void elements, wraps
//! code blocks in Rouge's div structure, and resolves entities to literal
//! characters.
//!
//! So pulldown-cmark supplies the event stream and this module supplies the
//! emitter, matching kramdown's `Converter::Html` output.
//!
//! Fidelity boundary: syntax-highlighted code blocks carry Rouge's token
//! `<span>`s, which would require porting Rouge's lexers; rekyll emits the
//! exact wrapper with escaped, untokenized content. Kramdown-only syntax
//! (inline attribute lists, definition lists, abbreviations, math) and a few
//! parser-level quirks (kramdown merges adjacent lists that use different
//! bullet markers) are not reproduced.

use crate::site::Site;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

pub fn convert(site: &Site, content: &str) -> String {
    convert_opts(content, smart_quotes(site))
}

/// Whether `kramdown.smart_quotes` enables typographic substitution.
pub fn smart_quotes(site: &Site) -> bool {
    site.config
        .get("kramdown")
        .and_then(|k| k.get("smart_quotes"))
        .map(|v| v.truthy())
        .unwrap_or(true)
}

/// Convert without needing a `Site`, for the `markdownify` filter.
pub fn convert_opts(content: &str, smart: bool) -> String {
    Emitter::new(content, smart).run()
}

/// The `smartify` filter: typographic substitution with no block parsing.
pub fn smartify_text(text: &str) -> String {
    smartify(text)
}

/// Kramdown's default `syntax_highlighter_opts.default_lang`, which Jekyll
/// sets to "plaintext"; it reaches the output as a CSS class.
const DEFAULT_LANG: &str = "plaintext";

struct Emitter<'a> {
    source: &'a str,
    smart: bool,
    out: String,
    /// Current block nesting depth; kramdown indents two spaces per level.
    indent: usize,
    id_counts: HashMap<String, i32>,
}

impl<'a> Emitter<'a> {
    fn new(source: &'a str, smart: bool) -> Emitter<'a> {
        Emitter {
            source,
            smart,
            out: String::with_capacity(source.len() * 2),
            indent: 0,
            id_counts: HashMap::new(),
        }
    }

    fn run(mut self) -> String {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_FOOTNOTES);
        options.insert(Options::ENABLE_STRIKETHROUGH);

        let parser = Parser::new_ext(self.source, options);
        let events: Vec<(Event, std::ops::Range<usize>)> =
            parser.into_offset_iter().collect();

        self.blocks(&events, 0, events.len());

        // A blank line at the end of the source is a :blank element like any
        // other, so it contributes a trailing newline. This also covers link
        // reference definitions, which emit nothing themselves but leave the
        // blank line before them behind.
        let last_end = events.iter().map(|(_, r)| r.end).max().unwrap_or(0);
        if blank_line_between(self.source, last_end, self.source.len()) {
            self.out.push('\n');
        }
        self.out
    }

    fn pad(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    /// Emit a run of sibling blocks, reinstating the blank lines that
    /// separated them in the source. Kramdown keeps blank lines as `:blank`
    /// elements and converts each run to a single newline.
    fn blocks(&mut self, events: &[(Event, std::ops::Range<usize>)], start: usize, end: usize) {
        let mut i = start;
        let mut prev_end: Option<usize> = None;

        while i < end {
            let (_, range) = &events[i];
            if let Some(pe) = prev_end {
                if pe <= range.start && blank_line_between(self.source, pe, range.start) {
                    self.out.push('\n');
                }
            }
            let next = self.block(events, i, end);
            prev_end = Some(range.end);
            i = next;
        }
    }

    /// Emit the block starting at `i`, returning the index just past it.
    fn block(&mut self, events: &[(Event, std::ops::Range<usize>)], i: usize, end: usize) -> usize {
        let (event, _) = &events[i];
        match event {
            Event::Start(Tag::Paragraph) => {
                let close = matching_end(events, i, end);
                self.pad();
                self.out.push_str("<p>");
                self.inlines(events, i + 1, close);
                self.out.push_str("</p>\n");
                close + 1
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let close = matching_end(events, i, end);
                let text = raw_text(events, i + 1, close);
                let id = self.header_id(&text);
                let n = heading_number(*level);
                self.pad();
                self.out.push_str(&format!("<h{n} id=\"{}\">", escape_attr(&id)));
                self.inlines(events, i + 1, close);
                self.out.push_str(&format!("</h{n}>\n"));
                close + 1
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let close = matching_end(events, i, end);
                self.pad();
                self.out.push_str("<blockquote>\n");
                self.indent += 1;
                self.blocks(events, i + 1, close);
                self.indent -= 1;
                self.pad();
                self.out.push_str("</blockquote>\n");
                close + 1
            }
            Event::Start(Tag::List(first)) => {
                let close = matching_end(events, i, end);
                let ordered = first.is_some();
                self.pad();
                match first {
                    // Kramdown omits start="1"; any other start is emitted.
                    Some(n) if *n != 1 => self.out.push_str(&format!("<ol start=\"{n}\">\n")),
                    Some(_) => self.out.push_str("<ol>\n"),
                    None => self.out.push_str("<ul>\n"),
                }
                self.indent += 1;
                self.list_items(events, i + 1, close);
                self.indent -= 1;
                self.pad();
                self.out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
                close + 1
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let close = matching_end(events, i, end);
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let first = info.split_whitespace().next().unwrap_or("");
                        if first.is_empty() {
                            DEFAULT_LANG.to_string()
                        } else {
                            first.to_string()
                        }
                    }
                    CodeBlockKind::Indented => DEFAULT_LANG.to_string(),
                };
                let mut body = String::new();
                for (e, _) in &events[i + 1..close] {
                    if let Event::Text(t) = e {
                        body.push_str(t);
                    }
                }
                self.code_block(&lang, &body);
                close + 1
            }
            Event::Start(Tag::Table(alignments)) => {
                let close = matching_end(events, i, end);
                let alignments = alignments.clone();
                self.table(events, i + 1, close, &alignments);
                close + 1
            }
            Event::Rule => {
                self.pad();
                self.out.push_str("<hr />\n");
                i + 1
            }
            Event::Html(html) => {
                // Block-level raw HTML passes through as written.
                self.out.push_str(html);
                i + 1
            }
            Event::Start(Tag::HtmlBlock) => {
                let close = matching_end(events, i, end);
                for (e, _) in &events[i + 1..close] {
                    if let Event::Html(h) | Event::Text(h) = e {
                        self.out.push_str(h);
                    }
                }
                close + 1
            }
            // Anything else is treated as inline content in an implicit block.
            _ => {
                self.inlines(events, i, i + 1);
                i + 1
            }
        }
    }

    fn list_items(&mut self, events: &[(Event, std::ops::Range<usize>)], start: usize, end: usize) {
        let mut i = start;
        while i < end {
            if !matches!(events[i].0, Event::Start(Tag::Item)) {
                i += 1;
                continue;
            }
            let close = matching_end(events, i, end);
            // A "tight" item holds inline content directly; a "loose" one wraps
            // it in a paragraph, and kramdown then puts the content on its own
            // indented lines.
            let loose = matches!(events.get(i + 1).map(|(e, _)| e), Some(Event::Start(Tag::Paragraph)));

            self.pad();
            if loose {
                self.out.push_str("<li>\n");
                self.indent += 1;
                self.blocks(events, i + 1, close);
                self.indent -= 1;
                self.pad();
                self.out.push_str("</li>\n");
            } else {
                self.out.push_str("<li>");
                // Inline content up to the first nested block, then any nested
                // list indented beneath it.
                let split = first_block_index(events, i + 1, close).unwrap_or(close);
                self.inlines(events, i + 1, split);
                if split < close {
                    self.out.push('\n');
                    self.indent += 1;
                    self.blocks(events, split, close);
                    self.indent -= 1;
                    self.pad();
                }
                self.out.push_str("</li>\n");
            }
            i = close + 1;
        }
    }

    fn code_block(&mut self, lang: &str, body: &str) {
        self.pad();
        self.out.push_str(&format!(
            "<div class=\"language-{} highlighter-rouge\"><div class=\"highlight\"><pre class=\"highlight\"><code>",
            escape_attr(lang)
        ));
        self.out.push_str(&escape_html(body));
        self.out.push_str("</code></pre></div></div>\n");
    }

    fn table(
        &mut self,
        events: &[(Event, std::ops::Range<usize>)],
        start: usize,
        end: usize,
        alignments: &[pulldown_cmark::Alignment],
    ) {
        self.pad();
        self.out.push_str("<table>\n");
        self.indent += 1;

        let mut i = start;
        let mut col = 0usize;
        let mut in_head = false;
        while i < end {
            match &events[i].0 {
                Event::Start(Tag::TableHead) => {
                    in_head = true;
                    self.pad();
                    self.out.push_str("<thead>\n");
                    self.indent += 1;
                    self.pad();
                    self.out.push_str("<tr>\n");
                    self.indent += 1;
                    col = 0;
                    i += 1;
                }
                Event::End(TagEnd::TableHead) => {
                    self.indent -= 1;
                    self.pad();
                    self.out.push_str("</tr>\n");
                    self.indent -= 1;
                    self.pad();
                    self.out.push_str("</thead>\n");
                    self.pad();
                    self.out.push_str("<tbody>\n");
                    self.indent += 1;
                    in_head = false;
                    i += 1;
                }
                Event::Start(Tag::TableRow) => {
                    self.pad();
                    self.out.push_str("<tr>\n");
                    self.indent += 1;
                    col = 0;
                    i += 1;
                }
                Event::End(TagEnd::TableRow) => {
                    self.indent -= 1;
                    self.pad();
                    self.out.push_str("</tr>\n");
                    i += 1;
                }
                Event::Start(Tag::TableCell) => {
                    let close = matching_end(events, i, end);
                    let tag = if in_head { "th" } else { "td" };
                    self.pad();
                    self.out.push_str(&format!("<{tag}{}>", align_attr(alignments.get(col))));
                    self.inlines(events, i + 1, close);
                    self.out.push_str(&format!("</{tag}>\n"));
                    col += 1;
                    i = close + 1;
                }
                _ => i += 1,
            }
        }

        self.indent -= 1;
        self.pad();
        self.out.push_str("</tbody>\n");
        self.indent -= 1;
        self.pad();
        self.out.push_str("</table>\n");
    }

    fn inlines(&mut self, events: &[(Event, std::ops::Range<usize>)], start: usize, end: usize) {
        let mut i = start;
        while i < end {
            match &events[i].0 {
                Event::Text(_) => {
                    // Consecutive text events are processed as one run against
                    // the original source. pulldown splits text at "<" and
                    // decodes entities eagerly, but kramdown applies its
                    // typographic substitutions across the whole run and keeps
                    // entities as separate nodes exempt from them.
                    let mut j = i;
                    while j < end && matches!(events[j].0, Event::Text(_)) {
                        j += 1;
                    }
                    let from = events[i].1.start;
                    let to = events[j - 1].1.end;
                    let raw = self.source.get(from..to);
                    match raw {
                        Some(raw) => self.out.push_str(&self.render_text_run(raw)),
                        None => {
                            for (e, _) in &events[i..j] {
                                if let Event::Text(t) = e {
                                    self.out.push_str(&escape_text(t));
                                }
                            }
                        }
                    }
                    i = j;
                    continue;
                }
                Event::Code(c) => {
                    self.out.push_str(&format!(
                        "<code class=\"language-{DEFAULT_LANG} highlighter-rouge\">{}</code>",
                        escape_html(c)
                    ));
                }
                Event::Html(h) | Event::InlineHtml(h) => {
                    self.out.push_str(&rewrite_inline_html(h));
                }
                Event::SoftBreak => self.out.push('\n'),
                Event::HardBreak => self.out.push_str("<br />\n"),
                Event::Start(Tag::Emphasis) => self.out.push_str("<em>"),
                Event::End(TagEnd::Emphasis) => self.out.push_str("</em>"),
                Event::Start(Tag::Strong) => self.out.push_str("<strong>"),
                Event::End(TagEnd::Strong) => self.out.push_str("</strong>"),
                Event::Start(Tag::Strikethrough) => self.out.push_str("<del>"),
                Event::End(TagEnd::Strikethrough) => self.out.push_str("</del>"),
                Event::Start(Tag::Link { dest_url, title, .. }) => {
                    self.out.push_str(&format!("<a href=\"{}\"", escape_attr(dest_url)));
                    if !title.is_empty() {
                        self.out.push_str(&format!(" title=\"{}\"", escape_attr(title)));
                    }
                    self.out.push('>');
                }
                Event::End(TagEnd::Link) => self.out.push_str("</a>"),
                Event::Start(Tag::Image { dest_url, title, .. }) => {
                    // Kramdown renders the alt text as a plain attribute.
                    let close = matching_end(events, i, end);
                    let alt = raw_text(events, i + 1, close);
                    self.out.push_str(&format!(
                        "<img src=\"{}\" alt=\"{}\"",
                        escape_attr(dest_url),
                        escape_attr(&alt)
                    ));
                    if !title.is_empty() {
                        self.out.push_str(&format!(" title=\"{}\"", escape_attr(title)));
                    }
                    self.out.push_str(" />");
                    i = close + 1;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Render a run of source text: resolve backslash escapes, apply
    /// typographic substitutions, and turn entity references into the literal
    /// characters `entity_output: as_char` asks for.
    fn render_text_run(&self, raw: &str) -> String {
        let entity = regex::Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[A-Za-z][A-Za-z0-9]*);").unwrap();

        let mut out = String::with_capacity(raw.len());
        let mut last = 0;
        for caps in entity.captures_iter(raw) {
            let whole = caps.get(0).unwrap();
            out.push_str(&self.plain_segment(&raw[last..whole.start()]));
            match decode_entity(&caps[1]) {
                // `<`, `>` and `&` stay as entities even in as_char mode,
                // because emitting them literally would break the markup.
                Some('<') => out.push_str("&lt;"),
                Some('>') => out.push_str("&gt;"),
                Some('&') => out.push_str("&amp;"),
                Some(c) => out.push(c),
                None => out.push_str(&escape_text(whole.as_str())),
            }
            last = whole.end();
        }
        out.push_str(&self.plain_segment(&raw[last..]));
        out
    }

    fn plain_segment(&self, segment: &str) -> String {
        let unescaped = unescape_backslashes(segment);
        let text = if self.smart { smartify(&unescaped) } else { unescaped };
        escape_text(&text)
    }

    /// `Kramdown::Parser::GFM#generate_gfm_header_id`: downcase, drop
    /// everything that is not a word character, hyphen, space or tab, then
    /// turn spaces and tabs into hyphens. Repeats gain a numeric suffix.
    fn header_id(&mut self, text: &str) -> String {
        let lowered = text.to_lowercase();
        let stripped: String = lowered
            .chars()
            .filter(|c| is_word_char(*c) || *c == '-' || *c == ' ' || *c == '\t')
            .collect();
        let result: String =
            stripped.chars().map(|c| if c == ' ' || c == '\t' { '-' } else { c }).collect();

        let counter = self.id_counts.entry(result.clone()).or_insert(-1);
        *counter += 1;
        if *counter > 0 {
            format!("{result}-{counter}")
        } else {
            result
        }
    }
}

/// Ruby's `\p{Word}`: letters, marks, digits and connector punctuation.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || unicode_is_mark(c)
}

fn unicode_is_mark(c: char) -> bool {
    // Combining marks occupy these ranges in practice for Latin text.
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x20D0..=0x20FF)
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn align_attr(a: Option<&pulldown_cmark::Alignment>) -> String {
    use pulldown_cmark::Alignment::*;
    match a {
        Some(Left) => " style=\"text-align: left\"".into(),
        Some(Center) => " style=\"text-align: center\"".into(),
        Some(Right) => " style=\"text-align: right\"".into(),
        _ => String::new(),
    }
}

/// Index of the matching `End` event for the `Start` at `i`.
fn matching_end(events: &[(Event, std::ops::Range<usize>)], i: usize, end: usize) -> usize {
    let mut depth = 0usize;
    for (j, (e, _)) in events.iter().enumerate().take(end).skip(i) {
        match e {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
    }
    end.saturating_sub(1)
}

/// The first nested block element inside a tight list item, if any.
fn first_block_index(
    events: &[(Event, std::ops::Range<usize>)],
    start: usize,
    end: usize,
) -> Option<usize> {
    let mut i = start;
    while i < end {
        match &events[i].0 {
            Event::Start(Tag::List(_)) | Event::Start(Tag::BlockQuote(_)) => return Some(i),
            Event::Start(_) => i = matching_end(events, i, end) + 1,
            _ => i += 1,
        }
    }
    None
}

/// Plain text of a range, used for header ids and image alt attributes.
fn raw_text(events: &[(Event, std::ops::Range<usize>)], start: usize, end: usize) -> String {
    let mut s = String::new();
    for (e, _) in &events[start..end.min(events.len())] {
        match e {
            Event::Text(t) | Event::Code(t) => s.push_str(t),
            Event::SoftBreak | Event::HardBreak => s.push(' '),
            _ => {}
        }
    }
    s
}

/// Was there a blank line in the source between two sibling blocks?
///
/// A block's range can run past its own text to the start of the next one, so
/// the gap is measured from the end of the previous block's actual content.
fn blank_line_between(source: &str, prev_end: usize, next_start: usize) -> bool {
    if prev_end > source.len() || next_start > source.len() {
        return false;
    }
    let content_end = source[..prev_end].trim_end().len();
    if content_end >= next_start {
        return false;
    }
    source[content_end..next_start].matches('\n').count() >= 2
}

/// Kramdown text escaping: `<` and `>` and bare `&` become entities, but a
/// well-formed entity reference is left intact and `"` stays literal.
fn escape_text(s: &str) -> String {
    escape_html(s)
}

/// Escaping inside `<pre>`/`<code>` blocks: `"` is left as written.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Kramdown emits XHTML, so raw void elements written as HTML gain a slash.
fn rewrite_inline_html(html: &str) -> String {
    let re = regex::Regex::new(
        r"(?i)<(area|base|br|col|embed|hr|img|input|link|meta|param|source|track|wbr)(\s[^>]*?)?\s*/?>",
    )
    .unwrap();
    re.replace_all(html, |c: &regex::Captures| {
        let name = &c[1];
        let attrs = c.get(2).map(|m| m.as_str().trim_end()).unwrap_or("");
        format!("<{name}{attrs} />")
    })
    .to_string()
}

/// Kramdown's typographic substitutions with `entity_output: as_char`.
fn smartify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        match c {
            '-' if chars.get(i + 1) == Some(&'-') && chars.get(i + 2) == Some(&'-') => {
                out.push('\u{2014}');
                i += 3;
                continue;
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                out.push('\u{2013}');
                i += 2;
                continue;
            }
            '.' if chars.get(i + 1) == Some(&'.') && chars.get(i + 2) == Some(&'.') => {
                out.push('\u{2026}');
                i += 3;
                continue;
            }
            // Kramdown binds a non-breaking space to a guillemet, absorbing
            // the space that separated it from the quoted text.
            '<' if chars.get(i + 1) == Some(&'<') => {
                out.push('\u{00AB}');
                i += 2;
                if chars.get(i) == Some(&' ') {
                    out.push('\u{00A0}');
                    i += 1;
                }
                continue;
            }
            ' ' if chars.get(i + 1) == Some(&'>') && chars.get(i + 2) == Some(&'>') => {
                out.push('\u{00A0}');
                out.push('\u{00BB}');
                i += 3;
                continue;
            }
            '>' if chars.get(i + 1) == Some(&'>') => {
                out.push('\u{00BB}');
                i += 2;
                continue;
            }
            '"' => {
                // An opening quote follows whitespace or nothing.
                let opening = prev.is_none_or(|p| p.is_whitespace() || "([{-\u{2013}\u{2014}".contains(p));
                out.push(if opening { '\u{201C}' } else { '\u{201D}' });
                i += 1;
                continue;
            }
            '\'' => {
                let opening = prev.is_none_or(|p| p.is_whitespace() || "([{-\u{2013}\u{2014}".contains(p));
                out.push(if opening { '\u{2018}' } else { '\u{2019}' });
                i += 1;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// Markdown backslash escapes: a backslash before ASCII punctuation is
/// dropped and the character taken literally.
fn unescape_backslashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(n) if n.is_ascii_punctuation() => {
                    out.push(*n);
                    chars.next();
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Resolve an HTML entity reference to its character.
fn decode_entity(body: &str) -> Option<char> {
    if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(dec) = body.strip_prefix('#') {
        return dec.parse::<u32>().ok().and_then(char::from_u32);
    }
    Some(match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "laquo" => '\u{00AB}',
        "raquo" => '\u{00BB}',
        "deg" => '\u{00B0}',
        "middot" => '\u{00B7}',
        "bull" => '\u{2022}',
        "dagger" => '\u{2020}',
        "para" => '\u{00B6}',
        "sect" => '\u{00A7}',
        "euro" => '\u{20AC}',
        "pound" => '\u{00A3}',
        "yen" => '\u{00A5}',
        "cent" => '\u{00A2}',
        _ => return None,
    })
}
