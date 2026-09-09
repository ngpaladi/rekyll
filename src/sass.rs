//! Sass/SCSS compilation.
//!
//! jekyll-sass-converter 2.2 uses sassc (libsass), whose default output style
//! is `:compact` — one line per top-level block, a blank line between blocks.
//! grass implements Sass faithfully but only offers `expanded` and
//! `compressed`, so its expanded output is reformatted here.
//!
//! Fidelity boundary: source maps are not reproduced. Jekyll emits a
//! `.css.map` alongside the stylesheet and appends a `sourceMappingURL`
//! comment unless `sass: {sourcemap: never}` is configured; with that setting
//! rekyll's CSS is byte-identical.

use crate::site::Site;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// The `sass:` settings a compile needs, so the filters can compile without a
/// whole `Site`.
#[derive(Debug, Clone)]
pub struct Options {
    pub load_paths: Vec<PathBuf>,
    pub style: String,
}

impl Options {
    pub fn from_site(site: &Site) -> Options {
        let sass = site.config.get("sass");
        let setting = |key, default: &str| {
            sass.and_then(|s| s.get(key)).and_then(crate::value::Value::as_str).unwrap_or(default).to_string()
        };
        let mut load_paths = vec![site.source.join(setting("sass_dir", "_sass"))];
        for p in crate::document::string_list(sass.and_then(|s| s.get("load_paths"))) {
            load_paths.push(site.source.join(p));
        }
        Options { load_paths, style: setting("style", "compact").trim_start_matches(':').to_string() }
    }
}

/// Compile SCSS/Sass source, honouring `sass.load_paths` and `sass.style`.
pub fn compile(site: &Site, source: &str, indented: bool) -> Result<String> {
    compile_with(&Options::from_site(site), source, indented)
}

pub fn compile_with(opts: &Options, source: &str, indented: bool) -> Result<String> {
    let compressed = opts.style == "compressed";
    let mut options = grass::Options::default()
        .style(if compressed { grass::OutputStyle::Compressed } else { grass::OutputStyle::Expanded });
    for p in &opts.load_paths {
        options = options.load_path(p);
    }
    if indented {
        options = options.input_syntax(grass::InputSyntax::Sass);
    }
    let css = grass::from_string(source.to_string(), &options).map_err(|e| anyhow!("Sass error: {e}"))?;
    // libsass's `:compact`, jekyll-sass-converter's default under sassc.
    let compact = !compressed && !matches!(opts.style.as_str(), "expanded" | "nested");
    Ok(if compact { to_compact(&css) } else { css })
}

/// Reformat expanded CSS as libsass's `:compact` style.
fn to_compact(css: &str) -> String {
    let items = top_level_items(css);
    let mut out = String::with_capacity(css.len());
    let mut prev_was_comment = false;

    for (i, item) in items.iter().enumerate() {
        let is_comment = item.starts_with("/*");
        // Blocks are separated by a blank line, but a comment stays attached
        // to the block that follows it.
        if i > 0 && !prev_was_comment {
            out.push('\n');
        }
        out.push_str(item);
        out.push('\n');
        prev_was_comment = is_comment;
    }
    out
}

/// Split expanded CSS into top-level blocks and comments, each collapsed onto
/// a single line. Empty rules are dropped, as libsass drops them.
fn top_level_items(css: &str) -> Vec<String> {
    let mut items = Vec::new();
    let bytes: Vec<char> = css.chars().collect();
    let mut i = 0;
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_string: Option<char> = None;

    while i < bytes.len() {
        let c = bytes[i];

        if let Some(q) = in_string {
            if c == q && bytes.get(i.wrapping_sub(1)) != Some(&'\\') {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match c {
            '"' | '\'' => in_string = Some(c),
            '/' if depth == 0 && bytes.get(i + 1) == Some(&'*') => {
                // A top-level comment is its own item.
                let text_before = collapse(&bytes[start..i].iter().collect::<String>());
                if !text_before.is_empty() {
                    push_item(&mut items, &text_before);
                }
                let end = find_comment_end(&bytes, i);
                push_item(&mut items, &bytes[i..end].iter().collect::<String>());
                i = end;
                start = end;
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let block: String = bytes[start..=i].iter().collect();
                    push_item(&mut items, &collapse(&block));
                    start = i + 1;
                }
            }
            // A statement such as @import or @charset ends at its semicolon.
            ';' if depth == 0 => {
                let stmt: String = bytes[start..=i].iter().collect();
                push_item(&mut items, &collapse(&stmt));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    let tail = collapse(&bytes[start.min(bytes.len())..].iter().collect::<String>());
    if !tail.is_empty() {
        push_item(&mut items, &tail);
    }
    items
}

fn push_item(items: &mut Vec<String>, item: &str) {
    let item = item.trim();
    if item.is_empty() {
        return;
    }
    // libsass omits rules with no declarations.
    if item.ends_with("{ }") || item.ends_with("{}") {
        return;
    }
    items.push(item.to_string());
}

fn find_comment_end(chars: &[char], from: usize) -> usize {
    let mut i = from + 2;
    while i + 1 < chars.len() {
        if chars[i] == '*' && chars[i + 1] == '/' {
            return i + 2;
        }
        i += 1;
    }
    chars.len()
}

/// Collapse a block's internal whitespace so it sits on one line.
fn collapse(block: &str) -> String {
    block.split_whitespace().collect::<Vec<_>>().join(" ")
}
