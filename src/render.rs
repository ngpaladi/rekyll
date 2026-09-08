//! Liquid rendering, payload assembly and the layout chain (`renderer.rb`).

use crate::config::deep_merge;
use crate::document::{document_to_liquid, Collection, Document};
use crate::lax::{LaxObject, LaxValue};
use crate::site::{Page, Site};
use crate::value::{Object, Value};
use anyhow::{anyhow, Result};
use std::collections::HashSet;

/// The Jekyll release whose behaviour this build reproduces. Templates read it
/// via `{{ jekyll.version }}`, so it has to match for identical output.
pub const JEKYLL_VERSION: &str = "4.3.2";

pub struct Renderer {
    parser: liquid::Parser,
}

impl Renderer {
    /// Build a parser bound to this site: `_includes` become Liquid partials
    /// and the URL index backs the `link` / `post_url` tags.
    pub fn new(site: &Site) -> Result<Renderer> {
        let mut source = liquid::partials::InMemorySource::new();
        for (name, content) in load_includes(site)? {
            source.add(name, content);
        }
        let partials = liquid::partials::EagerCompiler::new(source);

        let urls = std::sync::Arc::new(build_url_index(site));
        let baseurl = std::sync::Arc::new(site.config.str("baseurl").to_string());

        let parser = liquid::ParserBuilder::with_stdlib()
            .partials(partials)
            .tag(crate::tags::IncludeTag::new())
            .tag(crate::tags::IncludeTag::relative())
            .tag(crate::tags::LinkTag::new(urls.clone(), baseurl.clone()))
            .tag(crate::tags::LinkTag::post_url(urls, baseurl))
            .build()
            .map_err(|e| anyhow!("building Liquid parser: {e}"))?;
        Ok(Renderer { parser })
    }

    fn render_liquid(&self, template: &str, globals: &LaxObject, path: &str) -> Result<String> {
        let tpl = self
            .parser
            .parse(template)
            .map_err(|e| anyhow!("Liquid parse error in {path}: {e}"))?;
        tpl.render(globals)
            .map_err(|e| anyhow!("Liquid render error in {path}: {e}"))
    }

    /// `Renderer#run` for a page: Liquid, then the converter, then layouts.
    pub fn render_page(&self, site: &Site, page: &Page) -> Result<String> {
        let mut payload = site_payload(site);
        payload.insert("page", LaxValue::Object(LaxObject::from_value_object(&page_to_liquid(site, page))));
        payload.insert("paginator", LaxValue::Nil);

        // `assign_layout_data!` seeds `layout` from the page's own layout.
        let layout_data = page
            .data
            .get("layout")
            .and_then(Value::as_str)
            .and_then(|n| site.layouts.get(n))
            .map(|l| l.data.clone());
        payload.insert(
            "layout",
            match &layout_data {
                Some(d) => LaxValue::Object(LaxObject::from_value_object(d)),
                None => LaxValue::Nil,
            },
        );

        let mut output = page.content.clone();
        if render_with_liquid(&page.data, &output) {
            output = self.render_liquid(&output, &payload, &page.relative_path())?;
        }

        output = convert(site, page, &output);

        if place_in_layout(&page.data, &page.ext) {
            output = self.place_in_layouts(site, page, output, &mut payload)?;
        }
        Ok(output)
    }

    /// `Renderer#run` for a collection document.
    pub fn render_document(
        &self,
        site: &Site,
        collection: &Collection,
        doc: &Document,
    ) -> Result<String> {
        let mut payload = site_payload(site);
        let data = doc_to_liquid(site, collection, doc);
        payload.insert("page", LaxValue::Object(LaxObject::from_value_object(&data)));
        payload.insert("paginator", LaxValue::Nil);

        let layout_data = doc
            .data
            .get("layout")
            .and_then(Value::as_str)
            .and_then(|n| site.layouts.get(n))
            .map(|l| l.data.clone());
        payload.insert(
            "layout",
            match &layout_data {
                Some(d) => LaxValue::Object(LaxObject::from_value_object(d)),
                None => LaxValue::Nil,
            },
        );

        let mut output = doc.content.clone();
        if render_with_liquid(&doc.data, &output) {
            output = self.render_liquid(&output, &payload, &doc.relative_path)?;
        }
        if site.is_markdown(&doc.extname) {
            output = crate::markdown::convert(site, &output);
        }

        if place_in_layout(&doc.data, &doc.extname) {
            let page = pseudo_page(doc);
            output = self.place_in_layouts(site, &page, output, &mut payload)?;
        }
        Ok(output)
    }

    /// `Renderer#place_in_layouts`: walk the layout chain, stopping on a cycle.
    fn place_in_layouts(
        &self,
        site: &Site,
        page: &Page,
        content: String,
        payload: &mut LaxObject,
    ) -> Result<String> {
        let mut output = content;
        let mut name = page.data.get("layout").and_then(Value::as_str).map(str::to_string);

        if let Some(n) = &name {
            if !site.layouts.contains_key(n) {
                eprintln!(
                    "       Build Warning: Layout '{}' requested in {} does not exist.",
                    n,
                    page.relative_path()
                );
            }
        }

        // The payload's layout data is rebuilt from scratch for each page.
        let mut merged_layout = Object::new();
        payload.insert("layout", LaxValue::Nil);

        let mut used: HashSet<String> = HashSet::new();
        while let Some(current) = name.clone() {
            let layout = match site.layouts.get(&current) {
                Some(l) => l,
                None => break,
            };
            if !used.insert(current.clone()) {
                break;
            }

            // `render_layout`: content, then layout data merged over what the
            // chain has accumulated so far.
            payload.insert("content", LaxValue::str(output.clone()));
            merged_layout = deep_merge(&layout.data, &merged_layout);
            payload.insert("layout", LaxValue::Object(LaxObject::from_value_object(&merged_layout)));

            output = self.render_liquid(&layout.content, payload, &layout.path)?;

            name = layout.data.get("layout").and_then(Value::as_str).map(str::to_string);
        }
        Ok(output)
    }
}

/// `Utils.has_liquid_construct?` plus the front-matter opt-out.
fn render_with_liquid(data: &Object, content: &str) -> bool {
    if data.get("render_with_liquid").and_then(Value::as_bool) == Some(false) {
        return false;
    }
    !content.is_empty() && (content.contains("{%") || content.contains("{{"))
}

/// `Convertible#place_in_layout?`: Sass and CoffeeScript are asset files and
/// never get a layout, and `layout: none` opts out explicitly.
fn place_in_layout(data: &Object, ext: &str) -> bool {
    let asset = matches!(ext, ".sass" | ".scss" | ".coffee");
    let no_layout = data.get("layout").and_then(Value::as_str) == Some("none");
    !(asset || no_layout)
}

/// Run the matching converter over the content.
fn convert(site: &Site, page: &Page, content: &str) -> String {
    if site.is_markdown(&page.ext) {
        crate::markdown::convert(site, content)
    } else {
        content.to_string()
    }
}

/// A minimal `Page` standing in for a document, so the layout chain can be
/// driven by one code path.
fn pseudo_page(doc: &Document) -> Page {
    Page {
        dir: String::new(),
        name: doc.relative_path.clone(),
        basename: String::new(),
        ext: doc.extname.clone(),
        data: doc.data.clone(),
        content: String::new(),
        output: String::new(),
    }
}

/// `Document#to_liquid`.
pub fn doc_to_liquid(site: &Site, collection: &Collection, doc: &Document) -> Object {
    let url = site.doc_url(doc);
    document_to_liquid(doc, collection, &url, "", "")
}

/// `Page#to_liquid`: front matter deep-merged with the derived attributes.
pub fn page_to_liquid(site: &Site, page: &Page) -> Object {
    let url = site.page_url(page);
    let mut further = Object::new();
    further.insert("content".into(), Value::str(page.content.clone()));
    // `Page#dir` is the URL's directory, not the source directory.
    further.insert(
        "dir".into(),
        Value::str(if url.ends_with('/') {
            url.clone()
        } else {
            url_dir(&url)
        }),
    );
    further.insert("excerpt".into(), page.data.get("excerpt").cloned().unwrap_or(Value::Null));
    further.insert("name".into(), Value::str(page.name.clone()));
    further.insert("path".into(), Value::str(page.path()));
    further.insert("url".into(), Value::str(url));
    deep_merge(&page.data, &further)
}

fn url_dir(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..=i].to_string(),
        None => "/".to_string(),
    }
}

/// The `UnifiedPayloadDrop`: site, jekyll, and the per-page slots.
pub fn site_payload(site: &Site) -> LaxObject {
    let mut root = LaxObject::new();
    root.insert("site", LaxValue::Object(site_drop(site)));

    let mut jekyll = LaxObject::new();
    jekyll.insert("version", LaxValue::str(JEKYLL_VERSION));
    jekyll.insert(
        "environment",
        LaxValue::str(std::env::var("JEKYLL_ENV").unwrap_or_else(|_| "development".into())),
    );
    root.insert("jekyll", LaxValue::Object(jekyll));

    root.insert("content", LaxValue::Nil);
    root.insert("layout", LaxValue::Nil);
    root.insert("paginator", LaxValue::Nil);
    root
}

/// `SiteDrop`: the configuration as fallback data, with the computed
/// collections layered on top.
fn site_drop(site: &Site) -> LaxObject {
    let mut drop = LaxObject::from_value_object(&site.config.0);

    // `SiteDrop#config` deliberately returns nil.
    drop.insert("config", LaxValue::Nil);
    drop.insert("data", LaxValue::Object(LaxObject::from_value_object(&site.data)));
    drop.insert("time", LaxValue::str(site_time(site)));

    let pages: Vec<LaxValue> = site
        .pages
        .iter()
        .map(|p| LaxValue::Object(LaxObject::from_value_object(&page_to_liquid(site, p))))
        .collect();

    let html_pages: Vec<LaxValue> = site
        .pages
        .iter()
        .filter(|p| {
            let ext = site.output_ext(p);
            matches!(ext.as_str(), ".html" | ".xhtml" | ".htm") || site.page_url(p).ends_with('/')
        })
        .map(|p| LaxValue::Object(LaxObject::from_value_object(&page_to_liquid(site, p))))
        .collect();

    let static_files: Vec<LaxValue> = site
        .static_files
        .iter()
        .map(|f| {
            let mut o = LaxObject::new();
            o.insert("path", LaxValue::str(format!("/{}", f.relative_path())));
            o.insert("name", LaxValue::str(f.name.clone()));
            o.insert("basename", LaxValue::str(basename_no_ext(&f.name)));
            o.insert("extname", LaxValue::str(extname(&f.name)));
            LaxValue::Object(o)
        })
        .collect();

    drop.insert("pages", LaxValue::Array(pages));
    drop.insert("html_pages", LaxValue::Array(html_pages));
    drop.insert("static_files", LaxValue::Array(static_files));
    // `SiteDrop#posts` is newest-first, the reverse of the stored order.
    let posts_collection = site.collections.get("posts");
    let posts: Vec<LaxValue> = posts_collection
        .map(|c| {
            c.docs
                .iter()
                .rev()
                .map(|d| LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(site, c, d))))
                .collect()
        })
        .unwrap_or_default();
    drop.insert("posts", LaxValue::Array(posts));

    let mut documents = Vec::new();
    for (collection, doc) in site.documents() {
        documents.push(LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(
            site, collection, doc,
        ))));
    }
    drop.insert("documents", LaxValue::Array(documents));

    // `SiteDrop#[]` exposes each non-posts collection under its own label.
    for (label, collection) in &site.collections {
        if label == "posts" {
            continue;
        }
        let docs: Vec<LaxValue> = collection
            .docs
            .iter()
            .map(|d| {
                LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(site, collection, d)))
            })
            .collect();
        drop.insert(label.clone(), LaxValue::Array(docs));
    }

    // `SiteDrop#collections` is sorted by label.
    let mut labels: Vec<&String> = site.collections.keys().collect();
    labels.sort();
    let collections: Vec<LaxValue> = labels
        .iter()
        .map(|label| {
            let c = &site.collections[*label];
            let mut o = LaxObject::from_value_object(&c.metadata);
            o.insert("label", LaxValue::str((*label).clone()));
            o.insert("relative_directory", LaxValue::str(c.relative_directory()));
            o.insert(
                "docs",
                LaxValue::Array(
                    c.docs
                        .iter()
                        .map(|d| {
                            LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(
                                site, c, d,
                            )))
                        })
                        .collect(),
                ),
            );
            LaxValue::Object(o)
        })
        .collect();
    drop.insert("collections", LaxValue::Array(collections));

    drop.insert("tags", LaxValue::Object(group_by(site, "tags")));
    drop.insert("categories", LaxValue::Object(group_by(site, "categories")));
    drop.insert("related_posts", LaxValue::Nil);
    drop
}

/// `Site#time`: the pinned `time:` from configuration, else now.
fn site_time(site: &Site) -> String {
    if let Some(t) = site.config.get("time") {
        if t.truthy() {
            return t.to_string();
        }
    }
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z").to_string()
}

fn extname(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

fn basename_no_ext(name: &str) -> String {
    let e = extname(name);
    name[..name.len() - e.len()].to_string()
}

/// `Site#tags` / `Site#categories`: posts grouped by each value, with the
/// group keys in the order Jekyll's post traversal encounters them.
fn group_by(site: &Site, key: &str) -> LaxObject {
    let mut groups: indexmap::IndexMap<String, Vec<LaxValue>> = indexmap::IndexMap::new();
    if let Some(collection) = site.collections.get("posts") {
        for doc in &collection.docs {
            for value in crate::document::string_list(doc.data.get(key)) {
                groups.entry(value).or_default().push(LaxValue::Object(
                    LaxObject::from_value_object(&doc_to_liquid(site, collection, doc)),
                ));
            }
        }
    }
    let mut out = LaxObject::new();
    for (k, v) in groups {
        out.insert(k, LaxValue::Array(v));
    }
    out
}

/// Read `_includes` into partials keyed by their path within the directory.
/// `include_relative` targets are registered under a reserved prefix so both
/// tags can share one store.
fn load_includes(site: &Site) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();

    let dir = site.source.join(site.config.str("includes_dir"));
    if dir.is_dir() {
        for entry in walkdir::WalkDir::new(&dir).sort_by_file_name() {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry
                .path()
                .strip_prefix(&dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((name, std::fs::read_to_string(entry.path())?));
        }
    }

    // `include_relative` resolves against the including file's directory. The
    // whole source tree is registered so any relative target can be found.
    for entry in walkdir::WalkDir::new(&site.source).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(&site.source).unwrap().to_string_lossy().to_string();
        if rel.starts_with('_') && !rel.starts_with("_includes") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            out.push((format!("__relative__/{rel}"), text));
        }
    }
    Ok(out)
}

/// Source path to output URL, for `{% link %}` and `{% post_url %}`.
fn build_url_index(site: &Site) -> crate::tags::UrlIndex {
    let mut index = crate::tags::UrlIndex::new();
    for page in &site.pages {
        index.insert(page.relative_path(), site.page_url(page));
    }
    for (_, doc) in site.documents() {
        index.insert(doc.relative_path.clone(), site.doc_url(doc));
    }
    for file in &site.static_files {
        let rel = file.relative_path();
        index.insert(rel.clone(), format!("/{}", rel.trim_start_matches('/')));
    }
    index
}
