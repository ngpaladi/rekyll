//! Site reading, rendering and writing.
//!
//! Mirrors `jekyll/site.rb`, `reader.rb`, `page.rb` and `renderer.rb`.

use crate::config::{deep_merge, Config};
use crate::frontmatter;
use crate::url;
use crate::value::{Object, Value};
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
}

impl StaticFile {
    pub fn relative_path(&self) -> String {
        join_path(&self.dir, &self.name)
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
}

impl Site {
    pub fn new(source: &Path, dest: &Path) -> Result<Site> {
        let config = Config::load(source, dest)?;
        let markdown_exts = config
            .str("markdown_ext")
            .split(',')
            .map(|s| format!(".{}", s.trim()))
            .collect();
        Ok(Site {
            config,
            source: source.to_path_buf(),
            dest: dest.to_path_buf(),
            pages: Vec::new(),
            static_files: Vec::new(),
            layouts: HashMap::new(),
            data: Object::new(),
            markdown_exts,
        })
    }

    pub fn permalink_style(&self) -> String {
        self.config.str("permalink").to_string()
    }

    // -- reading ----------------------------------------------------------

    pub fn read(&mut self) -> Result<()> {
        self.read_layouts()?;
        self.read_directories("")?;
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
            });
        }
        Ok(())
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

        Ok(Page {
            dir: dir.to_string(),
            name: name.to_string(),
            basename,
            ext,
            data: parsed.data,
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

/// Deep-merge helper re-exported for the renderer's layout data handling.
pub fn merge(base: &Object, overlay: &Object) -> Object {
    deep_merge(base, overlay)
}

#[allow(dead_code)]
fn unused(_: &Value) {}
