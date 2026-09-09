//! Site reading, rendering and writing.
//!
//! Mirrors `jekyll/site.rb`, `reader.rb`, `page.rb` and `renderer.rb`.

use crate::config::{deep_merge, Config, Defaults};
use crate::document::{
    categories_from_path, generate_url_from_drop, pluralized, populate_title, Collection, Document,
    UrlDrop, DATE_FILENAME,
};
use crate::time::{parse_date, site_timezone, RTime};
use crate::url;
use crate::value::{Object, Value};
use chrono_tz::Tz;
use indexmap::IndexMap;
use anyhow::{Context, Result};
use globset::GlobBuilder;
use std::collections::HashMap;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// `Page::HTML_EXTENSIONS`.
const HTML_EXTENSIONS: &[&str] = &[".html", ".xhtml", ".htm"];

#[derive(Debug, Clone)]
pub struct Page {
    /// Directory between the source root and the file ("" at the root).
    pub dir: String,
    pub name: String,
    pub basename: String,
    /// File extension including the leading dot.
    pub ext: String,
    pub data: Object,
    pub content: String,
    pub output: String,
}

impl Page {
    pub fn relative_path(&self) -> String {
        join_path(&self.dir, &self.name).trim_start_matches('/').to_string()
    }

    /// `Convertible#path`: front matter may override it outright.
    pub fn path(&self) -> String {
        self.data.get("path").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| self.relative_path())
    }

    pub fn permalink(&self) -> Option<&str> {
        self.data.get("permalink").and_then(Value::as_str)
    }

    pub fn is_index(&self) -> bool {
        self.basename == "index"
    }
}

#[derive(Debug, Clone)]
pub struct StaticFile {
    pub name: String,
    pub source: PathBuf,
    /// The owning collection, when the file sits inside one. Such files are
    /// placed by the collection's URL template rather than by their path.
    pub collection: Option<String>,
    /// Path relative to the source root.
    pub relative_path: String,
}

impl StaticFile {
    /// `StaticFile#cleaned_relative_path`.
    fn cleaned_relative_path(&self, collection_dir: &str) -> String {
        let ext = split_ext(&self.name).1;
        let cleaned = self.relative_path[..self.relative_path.len() - ext.len()].trim_end_matches('.');
        cleaned.replacen(collection_dir, "", 1)
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub data: Object,
    pub content: String,
    pub path: String,
}

pub struct Site {
    pub config: Config,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub pages: Vec<Page>,
    pub static_files: Vec<StaticFile>,
    pub layouts: HashMap<String, Layout>,
    pub data: Object,
    pub markdown_exts: Vec<String>,
    pub collections: IndexMap<String, Collection>,
    pub timezone: Tz,
    pub defaults: Defaults,
    /// `Site#time`: the pinned `time:` from configuration, else process start.
    pub time: RTime,
}

impl Site {
    pub fn new(source: &Path, dest: &Path) -> Result<Site> {
        let config = Config::load(source, dest)?;
        let markdown_exts = config
            .str("markdown_ext")
            .split(',')
            .map(|s| format!(".{}", s.trim()))
            .collect();
        let timezone = site_timezone(config.get("timezone").and_then(Value::as_str));
        let time = match config.get("time").filter(|v| v.truthy()) {
            Some(v) => parse_date(&v.to_string(), timezone)?,
            None => crate::time::in_zone(chrono::Utc::now().naive_utc(), timezone),
        };

        // `Configuration#add_default_collections` guarantees "posts" exists.
        let mut collections = IndexMap::new();
        if let Some(defined) = config.get("collections").and_then(Value::as_object) {
            for (label, meta) in defined {
                collections.insert(
                    label.clone(),
                    Collection {
                        label: label.clone(),
                        metadata: meta.as_object().cloned().unwrap_or_default(),
                        docs: Vec::new(),
                    },
                );
            }
        }

        let defaults = Defaults::new(config.get("defaults"), config.str("collections_dir"));

        Ok(Site {
            config,
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            pages: Vec::new(),
            static_files: Vec::new(),
            layouts: HashMap::new(),
            data: Object::new(),
            markdown_exts,
            collections,
            timezone,
            defaults,
            time,
        })
    }

    pub fn permalink_style(&self) -> String {
        self.config.str("permalink").to_string()
    }

    // -- reading ----------------------------------------------------------

    pub fn read(&mut self) -> Result<()> {
        self.read_layouts()?;
        self.read_directories("")?;
        self.read_collections()?;
        self.read_data()?;
        for collection in self.collections.values_mut() {
            collection.docs.sort_by(|a, b| a.cmp_docs(b));
        }
        // `Reader#sort_files!`: pages by bare filename, static files by path.
        self.pages.sort_by(|a, b| a.name.cmp(&b.name));
        self.static_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(())
    }

    fn read_layouts(&mut self) -> Result<()> {
        let dir = self.source.join(self.config.str("layouts_dir"));
        if !dir.is_dir() {
            return Ok(());
        }
        for (rel, path) in walk_files(&dir)? {
            let parsed = self.read_front_matter(&path)?;
            // Layouts are keyed by path without extension, e.g. "post" or
            // "nested/post".
            let key = split_ext(&rel).0.to_string();
            let layout = Layout { data: parsed.data, content: parsed.content, path: path.to_string_lossy().to_string() };
            self.layouts.insert(key, layout);
        }
        Ok(())
    }

    /// The absolute path of a source-relative directory or file.
    fn abs(&self, rel: &str) -> PathBuf {
        self.source.join(rel.trim_start_matches('/'))
    }

    /// `Reader#read_directories`, recursive.
    fn read_directories(&mut self, dir: &str) -> Result<()> {
        let base = self.abs(dir);
        if !base.is_dir() {
            return Ok(());
        }
        let entries = self.filter_entries(&entry_names(&base)?, dir);

        let mut dirs = Vec::new();
        let mut pages = Vec::new();
        let mut statics = Vec::new();

        for entry in entries {
            let path = base.join(&entry);
            if path.is_dir() {
                dirs.push(entry);
            } else if has_yaml_header(&path) {
                pages.push(entry);
            } else {
                statics.push(entry);
            }
        }

        self.read_posts(dir)?;

        for d in dirs {
            let rel = join_path(dir, &d);
            // Never descend into the destination directory.
            if self.dest.canonicalize().ok() == base.join(&d).canonicalize().ok() {
                continue;
            }
            self.read_directories(&rel)?;
        }
        for name in pages {
            let page = self.read_page(dir, &name)?;
            self.pages.push(page);
        }
        for name in statics {
            self.static_files.push(StaticFile {
                source: base.join(&name),
                collection: None,
                relative_path: join_path(dir, &name),
                name,
            });
        }
        Ok(())
    }

    /// `Reader#retrieve_posts` for one directory: `<dir>/_posts` is read even
    /// though the entry filter hides underscore-prefixed directories.
    fn read_posts(&mut self, dir: &str) -> Result<()> {
        let posts_dir = self.abs(dir).join("_posts");
        if !posts_dir.is_dir() {
            return Ok(());
        }
        for entry in entry_names(&posts_dir)? {
            // Only date-prefixed filenames become posts.
            if !DATE_FILENAME.is_match(&entry) {
                continue;
            }
            let path = posts_dir.join(&entry);
            if path.is_dir() {
                continue;
            }
            let relative = join_path(&join_path(dir, "_posts"), &entry);
            let doc = self.read_document(&path, &relative, "posts")?;
            if let Some(doc) = doc {
                if let Some(c) = self.collections.get_mut("posts") {
                    c.docs.push(doc);
                }
            }
        }
        Ok(())
    }

    /// `CollectionReader`: read every non-posts collection from
    /// `<collections_dir>/_<label>`.
    fn read_collections(&mut self) -> Result<()> {
        let labels: Vec<String> =
            self.collections.keys().filter(|l| *l != "posts").cloned().collect();

        for label in labels {
            let dir = self
                .source
                .join(self.config.str("collections_dir"))
                .join(format!("_{label}"));
            if !dir.is_dir() {
                continue;
            }
            let mut docs = Vec::new();
            let mut statics = Vec::new();
            for (rel, path) in walk_files(&dir)? {
                let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                if is_special(&name) || name.ends_with('~') {
                    continue;
                }
                let relative = format!("_{label}/{rel}");
                if has_yaml_header(&path) {
                    if let Some(doc) = self.read_document(&path, &relative, &label)? {
                        docs.push(doc);
                    }
                } else {
                    // Files without front matter ride along as static files,
                    // placed by the collection's URL template.
                    statics.push(StaticFile {
                        name,
                        source: path,
                        collection: Some(label.clone()),
                        relative_path: relative,
                    });
                }
            }
            if let Some(c) = self.collections.get_mut(&label) {
                c.docs = docs;
            }
            self.static_files.extend(statics);
        }
        Ok(())
    }

    /// Build a `Document`, or `None` if the publisher would drop it.
    fn read_document(
        &self,
        path: &Path,
        relative_path: &str,
        label: &str,
    ) -> Result<Option<Document>> {
        let parsed = self.read_front_matter(path)?;
        let mut data = deep_merge(&self.defaults.all(relative_path, label), &parsed.data);

        let extname = split_ext(relative_path).1.to_string();
        let basename = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let basename_without_ext = split_ext(&basename).0.to_string();

        // `categories_from_path` runs before front matter is merged, so
        // directory-derived categories come first and front matter adds to them.
        let special_dir = format!("_{label}");
        let path_categories =
            categories_from_path(relative_path, &special_dir, &basename_without_ext);

        populate_title(&mut data, relative_path, &basename_without_ext);

        // The filename date seeds `date` unless front matter already set one.
        let mut date_source = data.get("date").filter(|v| v.truthy()).map(|v| v.to_string());
        if date_source.is_none() {
            if let Some(c) = DATE_FILENAME.captures(relative_path) {
                date_source = Some(c[1].to_string());
            }
        }
        let date = match date_source {
            Some(s) => parse_date(&s, self.timezone).with_context(|| {
                format!("Document '{relative_path}' does not have a valid date")
            })?,
            None => self.time.clone(),
        };
        data.insert("date".into(), Value::str(date.to_s()));

        let mut categories = path_categories;
        for c in pluralized(&data, "category", "categories") {
            if !categories.contains(&c) {
                categories.push(c);
            }
        }
        data.insert(
            "categories".into(),
            Value::Array(categories.into_iter().map(Value::Str).collect()),
        );
        let tags = pluralized(&data, "tag", "tags");
        data.insert("tags".into(), Value::Array(tags.into_iter().map(Value::Str).collect()));

        let doc = Document {
            path: path.to_path_buf(),
            relative_path: relative_path.to_string(),
            collection: label.to_string(),
            data,
            content: parsed.content,
            extname,
            date,
            draft: false,
        };

        Ok(if self.publish(&doc) { Some(doc) } else { None })
    }

    /// Read a file and split off its front matter. Jekyll logs a YAML error
    /// and carries on with empty data unless `strict_front_matter` is set.
    fn read_front_matter(&self, path: &Path) -> Result<FrontMatter> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let (parsed, error) = parse_front_matter(&text);
        if let Some(e) = error {
            eprintln!("             Error: YAML Exception reading {}: {e}", path.display());
            if self.config.bool("strict_front_matter") {
                return Err(e);
            }
        }
        Ok(parsed)
    }

    /// `Publisher#publish?`.
    fn publish(&self, doc: &Document) -> bool {
        let published = doc.data.get("published").map(Value::truthy).unwrap_or(true);
        let can_publish = published || self.config.bool("unpublished");
        let hidden_in_future =
            !self.config.bool("future") && doc.date.timestamp() > self.time.timestamp();
        can_publish && !hidden_in_future
    }

    /// `DataReader`: load `_data` into `site.data`, keyed by path.
    fn read_data(&mut self) -> Result<()> {
        let dir = self.source.join(self.config.str("data_dir"));
        if !dir.is_dir() {
            return Ok(());
        }
        self.data = read_data_dir(&dir)?;
        Ok(())
    }

    // -- document URLs ----------------------------------------------------

    pub fn collection_of(&self, doc: &Document) -> &Collection {
        &self.collections[&doc.collection]
    }

    /// `Renderer#output_ext` for a document.
    pub fn doc_output_ext(&self, doc: &Document) -> String {
        self.converted_ext(doc.permalink(), &doc.extname)
    }

    /// `Document#url`.
    pub fn doc_url(&self, doc: &Document) -> String {
        let collection = self.collection_of(doc);
        let output_ext = self.doc_output_ext(doc);
        let drop = UrlDrop::new(doc, collection, &output_ext);
        let template = match doc.permalink() {
            Some(p) => p.to_string(),
            None => collection.url_template(&self.permalink_style()),
        };
        url::sanitize_url(&generate_url_from_drop(&template, &drop))
    }

    /// `Document#destination`.
    pub fn doc_destination(&self, doc: &Document) -> PathBuf {
        self.destination(&self.doc_url(doc), &self.doc_output_ext(doc), true)
    }

    /// `StaticFile#url`: inside a collection the file is placed by the
    /// collection's URL template, otherwise by its own path.
    pub fn static_file_url(&self, file: &StaticFile) -> String {
        let collection = match file.collection.as_ref().and_then(|l| self.collections.get(l)) {
            Some(c) => c,
            None => return format!("/{}", file.relative_path),
        };
        let (stem, ext) = split_ext(&file.name);
        let placeholders: Vec<(&str, Option<String>)> = vec![
            ("collection", Some(collection.label.clone())),
            ("path", Some(file.cleaned_relative_path(&collection.relative_directory()))),
            ("output_ext", Some(String::new())),
            ("name", Some(stem.trim_end_matches('.').to_string())),
            ("title", Some(String::new())),
        ];
        let template = collection.url_template(&self.permalink_style());
        let base = url::sanitize_url(&url::generate_url(&template, &placeholders));
        format!("{}{ext}", base.trim_end_matches('/'))
    }

    /// `StaticFile#destination`.
    pub fn static_file_destination(&self, file: &StaticFile) -> PathBuf {
        self.dest.join(url::unescape_path(&self.static_file_url(file)).trim_start_matches('/'))
    }

    /// Every document that should be written, in `site.documents` order.
    pub fn documents(&self) -> Vec<(&Collection, &Document)> {
        let mut out = Vec::new();
        for collection in self.collections.values() {
            for doc in &collection.docs {
                out.push((collection, doc));
            }
        }
        out
    }

    fn read_page(&self, dir: &str, name: &str) -> Result<Page> {
        let path = self.abs(dir).join(name);
        let parsed = self.read_front_matter(&path)?;

        // `Page#process`: extension, then basename with trailing dots stripped.
        let (stem, ext) = split_ext(name);
        let data = deep_merge(&self.defaults.all(&join_path(dir, name), "pages"), &parsed.data);

        Ok(Page {
            dir: dir.to_string(),
            name: name.to_string(),
            basename: stem.trim_end_matches('.').to_string(),
            ext: ext.to_string(),
            data,
            content: parsed.content,
            output: String::new(),
        })
    }

    /// `EntryFilter#filter`.
    fn filter_entries(&self, entries: &[String], dir: &str) -> Vec<String> {
        let include = self.config.list("include");
        let exclude = self.config.list("exclude");
        // `site.exclude - site.include`: an explicit include cancels an exclude.
        let effective_exclude: Vec<String> =
            exclude.into_iter().filter(|e| !include.contains(e)).collect();

        entries
            .iter()
            .filter(|e| {
                if e.ends_with('.') {
                    return false;
                }
                let rel = join_path(dir, e).trim_start_matches('/').to_string();
                let included = self.glob_include(&include, e);
                if self.glob_include(&effective_exclude, &rel) && !included {
                    return false;
                }
                if included {
                    return true;
                }
                !(is_special(e) || e.ends_with('~'))
            })
            .cloned()
            .collect()
    }

    /// `EntryFilter#glob_include?`.
    fn glob_include(&self, patterns: &[String], entry: &str) -> bool {
        let entry_with_source = self.source.join(entry);
        let entry_str = entry_with_source.to_string_lossy().to_string();
        let is_dir = entry_with_source.is_dir();

        patterns.iter().any(|pattern| {
            let pattern_with_source = self.source.join(pattern);
            let pattern_str = pattern_with_source.to_string_lossy().to_string();
            if fnmatch(&pattern_str, &entry_str) {
                return true;
            }
            if entry_str.starts_with(&pattern_str) {
                return true;
            }
            is_dir && pattern_str == format!("{entry_str}/")
        })
    }

    // -- rendering --------------------------------------------------------

    /// Does this extension route through the Markdown converter?
    pub fn is_markdown(&self, ext: &str) -> bool {
        self.markdown_exts.iter().any(|m| m.eq_ignore_ascii_case(ext))
    }

    /// `Renderer#output_ext`: an explicit permalink with a file extension
    /// wins, otherwise the converter for the source extension decides.
    pub fn output_ext(&self, page: &Page) -> String {
        self.converted_ext(page.permalink(), &page.ext)
    }

    fn converted_ext(&self, permalink: Option<&str>, ext: &str) -> String {
        if let Some(p) = permalink.filter(|p| !p.ends_with('/')) {
            let ext = split_ext(p.rsplit('/').next().unwrap_or(p)).1;
            if !ext.is_empty() {
                return ext.to_string();
            }
        }
        if self.is_markdown(ext) {
            ".html".to_string()
        } else if is_sass(ext) {
            ".css".to_string()
        } else {
            ext.to_string()
        }
    }

    /// `Page#template`.
    fn page_template(&self, page: &Page) -> String {
        let output_ext = self.output_ext(page);
        let is_html = HTML_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(&output_ext));
        if !is_html {
            "/:path/:basename:output_ext".to_string()
        } else if page.is_index() {
            "/:path/".to_string()
        } else {
            url::add_permalink_suffix("/:path/:basename", &self.permalink_style())
        }
    }

    /// `Page#url`.
    pub fn page_url(&self, page: &Page) -> String {
        let output_ext = self.output_ext(page);
        let placeholders: Vec<(&str, Option<String>)> = vec![
            ("path", Some(page.dir.clone())),
            ("basename", Some(page.basename.clone())),
            ("output_ext", Some(output_ext)),
        ];
        let template = match page.permalink() {
            Some(p) => p.to_string(),
            None => self.page_template(page),
        };
        url::sanitize_url(&url::generate_url(&template, &placeholders))
    }

    /// `Page#destination`. A directory URL gets "index" plus the output
    /// extension, where a document gets "index.html" outright.
    pub fn page_destination(&self, page: &Page) -> PathBuf {
        self.destination(&self.page_url(page), &self.output_ext(page), false)
    }

    /// Shared by pages, documents and collection static files.
    fn destination(&self, url_path: &str, output_ext: &str, doc_index: bool) -> PathBuf {
        let mut path = self.dest.join(url::unescape_path(url_path).trim_start_matches('/'));
        if url_path.ends_with('/') {
            if doc_index {
                return path.join("index.html");
            }
            path = path.join("index");
        }
        let s = path.to_string_lossy().to_string();
        if !s.ends_with(output_ext) {
            path = PathBuf::from(format!("{s}{output_ext}"));
        }
        path
    }
}

/// Ruby's `File.extname` and `File.basename(name, ".*")` in one: split on the
/// last dot, treating a leading dot (".htaccess") as part of the name.
pub fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

/// Every file under `dir`, sorted, as (path relative to `dir`, absolute path).
pub fn walk_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, entry.path().to_path_buf()));
        }
    }
    Ok(out)
}

/// The names in a directory, sorted the way `Dir.entries.sort` would.
fn entry_names(dir: &Path) -> Result<Vec<String>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    Ok(names)
}

/// Sass sources are converted to CSS.
pub fn is_sass(ext: &str) -> bool {
    matches!(ext, ".sass" | ".scss")
}

/// `EntryFilter#special?`: a leading ".", "_", "#" or "~" on the entry or its
/// basename.
fn is_special(entry: &str) -> bool {
    let leading = |s: &str| matches!(s.chars().next(), Some('.') | Some('_') | Some('#') | Some('~'));
    leading(entry) || leading(entry.rsplit('/').next().unwrap_or(entry))
}

fn has_yaml_header(path: &Path) -> bool {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8];
    let n = f.read(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]);
    FRONT_MATTER_START.is_match(&head)
}

/// Ruby `File.fnmatch?` with no flags: `*` crosses "/" separators.
fn fnmatch(pattern: &str, path: &str) -> bool {
    GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
        .map(|g| g.compile_matcher().is_match(path))
        .unwrap_or(false)
}

/// `PathManager.join`: join with a single "/", tolerating empty components.
pub fn join_path(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.to_string()
    } else if b.is_empty() {
        a.to_string()
    } else {
        format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
    }
}


/// Recursively read a `_data` directory. Files become entries keyed by their
/// basename; subdirectories become nested hashes.
fn read_data_dir(dir: &Path) -> Result<Object> {
    let mut out = Object::new();
    for name in entry_names(dir)? {
        let path = dir.join(&name);
        if path.is_dir() {
            out.insert(name, Value::Object(read_data_dir(&path)?));
            continue;
        }
        let (key, ext) = split_ext(&name);
        if !matches!(ext.to_lowercase().as_str(), ".yml" | ".yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        out.insert(key.to_string(), crate::yaml::load(&text)?);
    }
    Ok(out)
}

// --- Front matter (`Document::YAML_FRONT_MATTER_REGEXP`) ---

pub struct FrontMatter {
    pub data: Object,
    pub content: String,
}

// Jekyll's exact regex. (?s) is Ruby's /m (dot matches newline) and (?m)
// makes ^/$ per-line, which Ruby does by default. \s matches \r, so CRLF files
// parse; and the greedy \s* before $ swallows the blank line after the closing
// marker, which shows whenever a template prints a page's raw content.
static FRONT_MATTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?sm)\A(---\s*\n.*?\n?)^((---|\.\.\.)\s*$\n?)").unwrap());

/// `Utils.has_yaml_header?`: the cheap check that decides a file is a page.
static FRONT_MATTER_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A---\s*\r?\n").unwrap());

/// Split a file into front matter and content. A YAML error comes back
/// alongside empty data, since the content is still usable.
pub fn parse_front_matter(text: &str) -> (FrontMatter, Option<anyhow::Error>) {
    let Some(caps) = FRONT_MATTER.captures(text) else {
        return (FrontMatter { data: Object::new(), content: text.to_string() }, None);
    };
    let yaml_src = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let content = text[caps.get(0).unwrap().end()..].to_string();
    // A front-matter block holding only comments parses to nil, which Jekyll
    // turns into an empty hash rather than an error.
    let (data, error) = match crate::yaml::load(yaml_src) {
        Ok(v) => (v.as_object().cloned().unwrap_or_default(), None),
        Err(e) => (Object::new(), Some(e)),
    };
    (FrontMatter { data, content }, error)
}
