//! Collection documents: posts and user-defined collections.
//!
//! Ported from `jekyll/document.rb`, `collection.rb`, `readers/post_reader.rb`
//! and `drops/url_drop.rb`.

use crate::config::deep_merge;
use crate::slug::{slugify, titleize_slug};
use crate::time::RTime;
use crate::url;
use crate::value::{Object, Value};
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;

/// `Document::DATE_FILENAME_MATCHER`.
pub fn date_filename_matcher() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:.+/)??(\d{2,4}-\d{1,2}-\d{1,2})-([^/]*)(\.[^.]+)$").unwrap())
}

/// `Document::DATELESS_FILENAME_MATCHER`.
pub fn dateless_filename_matcher() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^(?:.+/)*(.*)(\.[^.]+)$").unwrap())
}

#[derive(Debug, Clone)]
pub struct Document {
    /// Absolute path to the source file.
    pub path: PathBuf,
    /// Path relative to the collections directory, e.g. `_posts/2020-01-01-x.md`.
    pub relative_path: String,
    /// The owning collection's label, e.g. `posts`.
    pub collection: String,
    pub data: Object,
    pub content: String,
    pub extname: String,
    pub date: RTime,
    pub draft: bool,
}

impl Document {
    pub fn basename(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }

    /// `File.basename(path, ".*")` — strips only the final extension.
    pub fn basename_without_ext(&self) -> String {
        let b = self.basename();
        match b.rfind('.') {
            Some(0) | None => b,
            Some(i) => b[..i].to_string(),
        }
    }

    /// `Document#cleaned_relative_path`: drop the extension and the
    /// collection's own directory, then strip trailing dots.
    pub fn cleaned_relative_path(&self, collection_dir: &str) -> String {
        let without_ext = &self.relative_path[..self.relative_path.len() - self.extname.len()];
        without_ext.replacen(collection_dir, "", 1).trim_end_matches('.').to_string()
    }

    pub fn permalink(&self) -> Option<&str> {
        self.data.get("permalink").and_then(Value::as_str)
    }

    /// `Document#<=>`: by date, then by path.
    pub fn cmp_docs(&self, other: &Document) -> std::cmp::Ordering {
        self.date
            .timestamp()
            .cmp(&other.date.timestamp())
            .then_with(|| self.path.cmp(&other.path))
    }
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub label: String,
    pub metadata: Object,
    pub docs: Vec<Document>,
}

impl Collection {
    /// `Collection#relative_directory`.
    pub fn relative_directory(&self) -> String {
        format!("_{}", self.label)
    }

    /// `Collection#write?`.
    pub fn write(&self) -> bool {
        self.metadata.get("output").map(Value::truthy).unwrap_or(false)
    }

    /// `Collection#url_template`.
    pub fn url_template(&self, permalink_style: &str) -> String {
        match self.metadata.get("permalink").and_then(Value::as_str) {
            Some(p) => p.to_string(),
            None => url::add_permalink_suffix("/:collection/:path", permalink_style),
        }
    }
}

/// The `UrlDrop`: the placeholder values a permalink template can reference.
pub struct UrlDrop {
    values: Object,
}

impl UrlDrop {
    pub fn new(doc: &Document, collection: &Collection, output_ext: &str) -> UrlDrop {
        let mut v = Object::new();
        let basename = doc.basename_without_ext();

        v.insert("collection".into(), Value::str(collection.label.clone()));
        v.insert(
            "path".into(),
            Value::str(doc.cleaned_relative_path(&collection.relative_directory())),
        );
        v.insert("output_ext".into(), Value::str(output_ext.to_string()));
        v.insert("name".into(), Value::str(slugify(&basename, "default", false)));

        // `title` prefers an explicit `slug:`, in pretty mode preserving case.
        let slug_source = doc.data.get("slug").and_then(Value::as_str);
        v.insert(
            "title".into(),
            Value::str(match slug_source {
                Some(s) => slugify(s, "pretty", true),
                None => slugify(&basename, "pretty", true),
            }),
        );
        v.insert(
            "slug".into(),
            Value::str(match slug_source {
                Some(s) => slugify(s, "default", false),
                None => slugify(&basename, "default", false),
            }),
        );

        // Categories join with "/" after de-duplication, preserving order.
        let cats = string_list(doc.data.get("categories"));
        v.insert("categories".into(), Value::str(join_unique(cats.iter().map(|c| c.to_lowercase()))));
        v.insert(
            "slugified_categories".into(),
            Value::str(join_unique(cats.iter().map(|c| slugify(c, "default", false)))),
        );

        let d = &doc.date;
        for (key, fmt) in [
            ("year", "%Y"),
            ("month", "%m"),
            ("day", "%d"),
            ("hour", "%H"),
            ("minute", "%M"),
            ("second", "%S"),
            ("i_day", "%-d"),
            ("i_month", "%-m"),
            ("short_month", "%b"),
            ("long_month", "%B"),
            ("short_year", "%y"),
            ("w_year", "%G"),
            ("week", "%V"),
            ("w_day", "%u"),
            ("short_day", "%a"),
            ("long_day", "%A"),
            ("y_day", "%j"),
        ] {
            v.insert(key.into(), Value::str(d.format(fmt)));
        }

        UrlDrop { values: v }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
}

/// `URL#generate_url_from_drop`: replace every `:key`, allowing a trailing
/// underscore to belong to either the key or the literal text after it.
pub fn generate_url_from_drop(template: &str, drop: &UrlDrop) -> String {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r":([a-z_]+)").unwrap());

    let mut out = String::with_capacity(template.len());
    let mut last = 0;
    for caps in re.captures_iter(template) {
        let whole = caps.get(0).unwrap();
        let name = &caps[1];
        // "/:month_:day" must read as :month followed by "_", so try the key
        // with its trailing underscore first, then without.
        let winner = if name.ends_with('_') {
            if drop.get(name).is_some() {
                Some(name.to_string())
            } else {
                let trimmed = name.trim_end_matches('_').to_string();
                drop.get(&trimmed).map(|_| trimmed)
            }
        } else {
            drop.get(name).map(|_| name.to_string())
        };

        out.push_str(&template[last..whole.start()]);
        match winner {
            Some(key) => {
                let value = drop.get(&key).map(|v| v.to_string()).unwrap_or_default();
                out.push_str(&url::escape_path(&value));
                // Anything the regex swallowed beyond the key stays literal.
                out.push_str(&name[key.len()..]);
            }
            // Jekyll raises here; leaving the token intact keeps the build
            // going and makes the bad placeholder visible in the output.
            None => out.push_str(whole.as_str()),
        }
        last = whole.end();
    }
    out.push_str(&template[last..]);
    out
}

/// `Utils.pluralized_array_from_hash`: accept `category:`/`categories:` as a
/// string (split on whitespace) or a list.
pub fn string_list(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter(|x| !x.is_null()).map(|x| x.to_string()).collect(),
        Some(Value::Str(s)) => s.split_whitespace().map(str::to_string).collect(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.to_string()],
    }
}

fn join_unique(items: impl Iterator<Item = String>) -> String {
    let mut seen = std::collections::HashSet::new();
    items.filter(|s| seen.insert(s.clone())).collect::<Vec<_>>().join("/")
}

/// `Document#populate_categories` and `#populate_tags` merged: normalise the
/// singular/plural forms into a de-duplicated list.
pub fn pluralized(data: &Object, singular: &str, plural: &str) -> Vec<String> {
    let mut out = string_list(data.get(singular));
    if out.is_empty() {
        out = string_list(data.get(plural));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

/// `Document#populate_title`: derive title, slug and ext from the filename.
pub fn populate_title(data: &mut Object, relative_path: &str, basename_without_ext: &str) {
    let (slug, ext) = if let Some(c) = date_filename_matcher().captures(relative_path) {
        (c[2].to_string(), Some(c[3].to_string()))
    } else if let Some(c) = dateless_filename_matcher().captures(relative_path) {
        (c[1].to_string(), Some(c[2].to_string()))
    } else {
        (basename_without_ext.to_string(), None)
    };
    let slug = slug.trim_end_matches('.').to_string();

    if !data.contains_key("title") || !data["title"].truthy() {
        data.insert("title".into(), Value::str(titleize_slug(&slug)));
    }
    if !data.contains_key("slug") || !data["slug"].truthy() {
        data.insert("slug".into(), Value::str(slug));
    }
    if let Some(ext) = ext {
        if !data.contains_key("ext") || !data["ext"].truthy() {
            data.insert("ext".into(), Value::str(ext));
        }
    }
}

/// `Document#categories_from_path`: directories between the collection root
/// and the file become categories.
pub fn categories_from_path(relative_path: &str, special_dir: &str, basename: &str) -> Vec<String> {
    if relative_path.starts_with(special_dir) {
        return Vec::new();
    }
    // Everything up to and including the special directory is dropped.
    let cut = match relative_path.find(special_dir) {
        Some(i) => &relative_path[..i],
        None => relative_path,
    };
    cut.split('/')
        .filter(|c| !c.is_empty() && *c != special_dir && *c != basename)
        .map(str::to_string)
        .collect()
}

/// `Document#to_liquid` via `DocumentDrop`.
pub fn document_to_liquid(
    doc: &Document,
    collection: &Collection,
    url: &str,
    output: &str,
    excerpt: &str,
) -> Object {
    let mut further = Object::new();
    further.insert("path".into(), Value::str(doc.relative_path.clone()));
    further.insert("relative_path".into(), Value::str(doc.relative_path.clone()));
    further.insert("url".into(), Value::str(url.to_string()));
    further.insert("collection".into(), Value::str(collection.label.clone()));
    further.insert("content".into(), Value::str(doc.content.clone()));
    further.insert("output".into(), Value::str(output.to_string()));
    further.insert("excerpt".into(), Value::str(excerpt.to_string()));
    further.insert("date".into(), Value::str(doc.date.to_s()));
    // `Document#id`: the URL's directory joined with the slug, so it carries
    // no file extension.
    let dirname = match url.trim_end_matches('/').rfind('/') {
        Some(i) if i > 0 => &url[..i],
        _ => "",
    };
    let id_slug = doc
        .data
        .get("slug")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| doc.basename_without_ext());
    further.insert("id".into(), Value::str(format!("{dirname}/{id_slug}")));
    further.insert(
        "name".into(),
        doc.data.get("name").cloned().unwrap_or_else(|| Value::str(doc.basename())),
    );
    further.insert("draft".into(), Value::Bool(doc.draft));
    deep_merge(&doc.data, &further)
}
