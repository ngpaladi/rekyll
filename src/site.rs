//! Site reading, rendering and writing.
//!
//! Mirrors `jekyll/site.rb`, `reader.rb`, `page.rb` and `renderer.rb`.

use crate::config::{deep_merge, Config};
use crate::defaults::Defaults;
use crate::document::{
    categories_from_path, date_filename_matcher, generate_url_from_drop,
    pluralized, populate_title, Collection, Document, UrlDrop,
};
use crate::frontmatter;
use crate::time::{parse_date, site_timezone, RTime};
use crate::url;
use crate::value::{Object, Value};
use chrono_tz::Tz;
use indexmap::IndexMap;
use anyhow::{Context, Result};
use globset::GlobBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `Page::HTML_EXTENSIONS`.
const HTML_EXTENSIONS: &[&str] = &[".html", ".xhtml", ".htm"];

#[derive(Debug, Clone)]
pub struct Page {
    /// Directory between the source root and the file, with a leading slash.
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
        frontmatter::data_str(&self.data, "path")
            .map(str::to_string)
            .unwrap_or_else(|| self.relative_path())
    }

    pub fn permalink(&self) -> Option<&str> {
        frontmatter::data_str(&self.data, "permalink")
    }

    pub fn is_index(&self) -> bool {
        self.basename == "index"
    }
}

#[derive(Debug, Clone)]
pub struct StaticFile {
    pub dir: String,
    pub name: String,
    pub source: PathBuf,
    /// The owning collection, when the file sits inside one. Such files are
    /// placed by the collection's URL template rather than by their path.
    pub collection: Option<String>,
    /// Path relative to the source root, used when a collection applies.
    pub relative: String,
}

impl StaticFile {
    pub fn relative_path(&self) -> String {
        if self.collection.is_some() {
            return self.relative.clone();
        }
        join_path(&self.dir, &self.name)
    }

    fn extname(&self) -> String {
        match self.name.rfind('.') {
            Some(i) if i > 0 => self.name[i..].to_string(),
            _ => String::new(),
        }
    }

    /// `StaticFile#basename`.
    fn basename(&self) -> String {
        let e = self.extname();
        self.name[..self.name.len() - e.len()].trim_end_matches('.').to_string()
    }

    /// `StaticFile#cleaned_relative_path`.
    fn cleaned_relative_path(&self, collection_dir: &str) -> String {
        let e = self.extname();
        let cleaned = self.relative[..self.relative.len() - e.len()].trim_end_matches('.');
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
        self.static_files.sort_by(|a, b| a.relative_path().cmp(&b.relative_path()));
        Ok(())
    }

    fn read_layouts(&mut self) -> Result<()> {
        let dir = self.source.join(self.config.str("layouts_dir"));
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in walkdir::WalkDir::new(&dir).sort_by_file_name() {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(&dir).unwrap();
            let text = std::fs::read_to_string(entry.path())
                .with_context(|| format!("reading layout {}", entry.path().display()))?;
            let parsed = frontmatter::parse(&text);
            // Layouts are keyed by path without extension, e.g. "post" or
            // "nested/post".
            let key = rel.with_extension("").to_string_lossy().replace('\\', "/");
            self.layouts.insert(
                key,
                Layout {
                    data: parsed.data,
                    content: parsed.content,
                    path: entry.path().to_string_lossy().to_string(),
                },
            );
        }
        Ok(())
    }

    /// `Reader#read_directories`, recursive.
    fn read_directories(&mut self, dir: &str) -> Result<()> {
        let base = if dir.is_empty() {
            self.source.clone()
        } else {
            self.source.join(dir.trim_start_matches('/'))
        };
        if !base.is_dir() {
            return Ok(());
        }

        let mut entries: Vec<String> = std::fs::read_dir(&base)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        let entries = self.filter_entries(&entries, dir);

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
                dir: dir.to_string(),
                name: name.clone(),
                source: base.join(&name),
                collection: None,
                relative: join_path(dir, &name).trim_start_matches('/').to_string(),
            });
        }
        Ok(())
    }


    /// `Reader#retrieve_posts` for one directory: `<dir>/_posts` is read even
    /// though the entry filter hides underscore-prefixed directories.
    fn read_posts(&mut self, dir: &str) -> Result<()> {
        let posts_dir = if dir.is_empty() {
            self.source.join("_posts")
        } else {
            self.source.join(dir.trim_start_matches('/')).join("_posts")
        };
        if !posts_dir.is_dir() {
            return Ok(());
        }

        let mut entries: Vec<String> = std::fs::read_dir(&posts_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();

        for entry in entries {
            // Only date-prefixed filenames become posts.
            if !date_filename_matcher().is_match(&entry) {
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
            for entry in walkdir::WalkDir::new(&dir).sort_by_file_name() {
                let entry = entry?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if is_special(&name) || name.ends_with('~') {
                    continue;
                }
                let rel_in_collection =
                    entry.path().strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
                let relative = format!("_{label}/{rel_in_collection}");

                if has_yaml_header(entry.path()) {
                    if let Some(doc) = self.read_document(entry.path(), &relative, &label)? {
                        docs.push(doc);
                    }
                } else {
                    // Files without front matter ride along as static files,
                    // placed by the collection's URL template.
                    statics.push(StaticFile {
                        dir: format!("/{}", parent_of(&relative)),
                        name,
                        source: entry.path().to_path_buf(),
                        collection: Some(label.clone()),
                        relative: relative.clone(),
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
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading document {}", path.display()))?;
        let parsed = frontmatter::parse(&text);
        let mut data = deep_merge(&self.defaults.all(relative_path, label), &parsed.data);

        let extname = Path::new(relative_path)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let basename = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let basename_without_ext = match basename.rfind('.') {
            Some(0) | None => basename.clone(),
            Some(i) => basename[..i].to_string(),
        };

        // `categories_from_path` runs before front matter is merged, so
        // directory-derived categories come first and front matter adds to them.
        let special_dir = format!("_{label}");
        let path_categories =
            categories_from_path(relative_path, &special_dir, &basename_without_ext);

        populate_title(&mut data, relative_path, &basename_without_ext);

        // The filename date seeds `date` unless front matter already set one.
        let mut date_source = data.get("date").filter(|v| v.truthy()).map(|v| v.to_string());
        if date_source.is_none() {
            if let Some(c) = date_filename_matcher().captures(relative_path) {
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
        if let Some(p) = doc.permalink() {
            if !p.ends_with('/') {
                if let Some(e) = Path::new(p).extension() {
                    return format!(".{}", e.to_string_lossy());
                }
            }
        }
        if self.is_markdown(&doc.extname) {
            ".html".to_string()
        } else {
            doc.extname.clone()
        }
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
        let u = self.doc_url(doc);
        let output_ext = self.doc_output_ext(doc);
        let mut path = self.dest.join(url::unescape_path(&u).trim_start_matches('/'));
        if u.ends_with('/') {
            // Documents get index.html, where pages get "index" plus the ext.
            return path.join("index.html");
        }
        let s = path.to_string_lossy().to_string();
        if !s.ends_with(&output_ext) {
            path = PathBuf::from(format!("{s}{output_ext}"));
        }
        path
    }

    /// `StaticFile#url`: inside a collection the file is placed by the
    /// collection's URL template, otherwise by its own path.
    pub fn static_file_url(&self, file: &StaticFile) -> String {
        let collection = match file.collection.as_ref().and_then(|l| self.collections.get(l)) {
            Some(c) => c,
            None => return format!("/{}", file.relative_path().trim_start_matches('/')),
        };
        let placeholders: Vec<(&str, Option<String>)> = vec![
            ("collection", Some(collection.label.clone())),
            ("path", Some(file.cleaned_relative_path(&collection.relative_directory()))),
            ("output_ext", Some(String::new())),
            ("name", Some(file.basename())),
            ("title", Some(String::new())),
        ];
        let template = collection.url_template(&self.permalink_style());
        let base = url::sanitize_url(&url::generate_url(&template, &placeholders));
        format!("{}{}", base.trim_end_matches('/'), file.extname())
    }

    /// `StaticFile#destination`.
    pub fn static_file_destination(&self, file: &StaticFile) -> PathBuf {
        let u = self.static_file_url(file);
        self.dest.join(url::unescape_path(&u).trim_start_matches('/'))
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
        let path = if dir.is_empty() {
            self.source.join(name)
        } else {
            self.source.join(dir.trim_start_matches('/')).join(name)
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading page {}", path.display()))?;
        let parsed = frontmatter::parse(&text);

        // `Page#process`: extension, then basename with trailing dots stripped.
        let ext = Path::new(name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let basename = name[..name.len() - ext.len()].trim_end_matches('.').to_string();

        let relative_path = join_path(dir, name).trim_start_matches('/').to_string();
        let data = deep_merge(&self.defaults.all(&relative_path, "pages"), &parsed.data);

        Ok(Page {
            dir: dir.to_string(),
            name: name.to_string(),
            basename,
            ext,
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
                let included = self.glob_include(&include, e) || self.glob_include(&include, e);
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

    /// `Renderer#output_ext`.
    pub fn output_ext(&self, page: &Page) -> String {
        // An explicit permalink with a file extension wins outright.
        if let Some(p) = page.permalink() {
            if !p.ends_with('/') {
                if let Some(e) = Path::new(p).extension() {
                    return format!(".{}", e.to_string_lossy());
                }
            }
        }
        if self.is_markdown(&page.ext) {
            ".html".to_string()
        } else if is_sass(&page.ext) {
            ".css".to_string()
        } else {
            page.ext.clone()
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

    /// `Page#destination`.
    pub fn page_destination(&self, page: &Page) -> PathBuf {
        let u = self.page_url(page);
        let output_ext = self.output_ext(page);
        let mut path = self.dest.join(url::unescape_path(&u).trim_start_matches('/'));
        if u.ends_with('/') {
            path = path.join("index");
        }
        let s = path.to_string_lossy().to_string();
        if !s.ends_with(&output_ext) {
            path = PathBuf::from(format!("{s}{output_ext}"));
        }
        path
    }
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
    frontmatter::has_yaml_header(&head)
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


fn parent_of(relative: &str) -> String {
    match relative.rfind('/') {
        Some(i) => relative[..i].to_string(),
        None => String::new(),
    }
}

/// Recursively read a `_data` directory. Files become entries keyed by their
/// basename; subdirectories become nested hashes.
fn read_data_dir(dir: &Path) -> Result<Object> {
    let mut out = Object::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            out.insert(name, Value::Object(read_data_dir(&path)?));
            continue;
        }
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "yml" | "yaml") {
            continue;
        }
        let key = name[..name.len() - ext.len() - 1].to_string();
        let text = std::fs::read_to_string(&path)?;
        out.insert(key, crate::yaml::load(&text)?);
    }
    Ok(out)
}
