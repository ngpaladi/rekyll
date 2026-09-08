//! Liquid rendering, payload assembly and the layout chain (`renderer.rb`).

use crate::config::deep_merge;
use crate::document::{document_to_liquid, Collection, Document};
use crate::lax::{LaxObject, LaxValue};
use crate::site::{Page, Site};
use crate::value::{Object, Value};
use liquid::ValueView;
use anyhow::{anyhow, Context, Result};
use std::collections::{HashMap, HashSet};

/// The Jekyll release whose behaviour this build reproduces. Templates read it
/// via `{{ jekyll.version }}`, so it has to match for identical output.
pub const JEKYLL_VERSION: &str = "4.3.2";

/// What a document exposes after being rendered.
///
/// `Renderer#render_document` assigns `document.content = output` once the
/// converter has run and before layouts, then stores the layout result as
/// `document.output`. `DocumentDrop` reads both live, so `{{ post.content }}`
/// in an index page yields converted HTML, not Markdown source. Jekyll renders
/// documents before pages and updates this state as it goes, so a document
/// rendered earlier is visible in its converted form to one rendered later.
#[derive(Default, Clone)]
pub struct Rendered {
    pub content: String,
    pub output: String,
    pub excerpt: String,
}

pub type RenderState = HashMap<String, Rendered>;

/// The site-wide half of the Liquid payload.
///
/// Building it means materialising every document and page as a Liquid object,
/// which is far too expensive to redo per template: Jekyll's payload is a Drop
/// that delegates lazily, so it costs nothing to hand to each render. Here it
/// is built once and shared by reference, then patched in place as documents
/// finish rendering, which keeps Jekyll's sequential visibility without the
/// quadratic rebuild.
pub struct Payload {
    site: std::sync::Arc<LaxValue>,
    jekyll: LaxValue,
    /// Where each document's object sits inside the drop. The drop exposes the
    /// same document several times over (posts, documents, the collection's
    /// key, collections[].docs, each tag and category group), so the locations
    /// are indexed once and updates go straight to them; searching the tree per
    /// update made a large site quadratic.
    doc_paths: HashMap<String, Vec<Vec<Step>>>,
}

/// One step along a path into the drop.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Index(usize),
}

impl Payload {
    pub fn new(site: &Site) -> Payload {
        let empty = RenderState::new();
        let mut jekyll = LaxObject::new();
        jekyll.insert("version", LaxValue::str(JEKYLL_VERSION));
        jekyll.insert(
            "environment",
            LaxValue::str(std::env::var("JEKYLL_ENV").unwrap_or_else(|_| "development".into())),
        );
        let site_value = LaxValue::Object(site_drop(site, &empty));
        let mut doc_paths = HashMap::new();
        index_documents(&site_value, &mut Vec::new(), &mut doc_paths);

        Payload {
            site: std::sync::Arc::new(site_value),
            jekyll: LaxValue::Object(jekyll),
            doc_paths,
        }
    }

    /// Reflect a finished document everywhere it appears in the drop, so a
    /// document rendered later sees its converted content.
    pub fn update_document(&mut self, relative_path: &str, rendered: &Rendered) {
        let paths = match self.doc_paths.get(relative_path) {
            Some(p) => p.clone(),
            None => return,
        };
        let site = std::sync::Arc::make_mut(&mut self.site);
        for path in &paths {
            if let Some(target) = follow_mut(site, path) {
                if let LaxValue::Object(obj) = target {
                    obj.insert("content", LaxValue::str(rendered.content.clone()));
                    obj.insert("output", LaxValue::str(rendered.output.clone()));
                    obj.insert("excerpt", LaxValue::str(rendered.excerpt.clone()));
                }
            }
        }
    }

    /// The per-render root: the shared site drop plus this page's own slots.
    fn root(&self) -> LaxObject {
        let mut root = LaxObject::new();
        root.insert("site", LaxValue::Shared(self.site.clone()));
        root.insert("jekyll", self.jekyll.clone());
        root.insert("content", LaxValue::Nil);
        root.insert("layout", LaxValue::Nil);
        root.insert("paginator", LaxValue::Nil);
        root
    }
}

/// Record where every document object appears, keyed by its relative path.
fn index_documents(
    value: &LaxValue,
    path: &mut Vec<Step>,
    out: &mut HashMap<String, Vec<Vec<Step>>>,
) {
    match value {
        LaxValue::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                path.push(Step::Index(i));
                index_documents(item, path, out);
                path.pop();
            }
        }
        LaxValue::Object(obj) => {
            if let Some(rp) = obj.0.get("relative_path") {
                // Documents are the only objects carrying relative_path, and
                // they never nest, so this subtree needs no further walking.
                out.entry(rp.to_kstr().to_string()).or_default().push(path.clone());
                return;
            }
            for (k, v) in obj.0.iter() {
                path.push(Step::Key(k.clone()));
                index_documents(v, path, out);
                path.pop();
            }
        }
        LaxValue::Shared(_) | LaxValue::Nil | LaxValue::Scalar(_) => {}
    }
}

fn follow_mut<'a>(value: &'a mut LaxValue, path: &[Step]) -> Option<&'a mut LaxValue> {
    let mut current = value;
    for step in path {
        current = match (current, step) {
            (LaxValue::Object(o), Step::Key(k)) => o.0.get_mut(k)?,
            (LaxValue::Array(a), Step::Index(i)) => a.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(current)
}

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

        let ctx = std::sync::Arc::new(crate::filters::FilterCtx {
            baseurl: site.config.str("baseurl").to_string(),
            url: site.config.str("url").to_string(),
            timezone: site.timezone,
            smart_quotes: crate::markdown::smart_quotes(site),
            site_time: site.time.clone(),
        });

        let mut builder = liquid::ParserBuilder::with_stdlib()
            .partials(partials)
            .tag(crate::tags::IncludeTag::new())
            .tag(crate::tags::IncludeTag::relative())
            .tag(crate::tags::LinkTag::new(urls.clone(), baseurl.clone()))
            .tag(crate::tags::LinkTag::post_url(urls, baseurl))
            .block(crate::tags::HighlightTag::new());

        // Registered after the stdlib so Jekyll's overrides win.
        for (name, func) in crate::filters::all() {
            builder = builder.filter(crate::filters::JekyllFilter::new(name, func, ctx.clone()));
        }

        let parser = builder.build().map_err(|e| anyhow!("building Liquid parser: {e}"))?;
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

    /// Render a bare Liquid string against the site payload. Used by the
    /// filter differential harness.
    pub fn render_string(&self, site: &Site, template: &str, _state: &RenderState) -> Result<String> {
        let payload = Payload::new(site).root();
        self.render_liquid(template, &payload, "<string>")
    }

    /// `Renderer#run` for a page: Liquid, then the converter, then layouts.
    pub fn render_page(&self, site: &Site, page: &Page, payload: &Payload) -> Result<String> {
        let mut payload = payload.root();
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

        output = convert(site, page, &output)
            .with_context(|| format!("converting {}", page.relative_path()))?;

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
        payload: &Payload,
        state: &RenderState,
    ) -> Result<(String, String)> {
        let mut payload_root = payload.root();
        let data = doc_to_liquid(site, collection, doc, state);
        let payload = &mut payload_root;
        #[allow(unused_mut)]
        let mut payload = payload;
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

        // `document.content` is reassigned here, before layouts run.
        let converted = output.clone();

        if place_in_layout(&doc.data, &doc.extname) {
            let page = pseudo_page(doc);
            output = self.place_in_layouts(site, &page, output, &mut payload)?;
        }
        Ok((converted, output))
    }

    /// `Jekyll::Excerpt`: the content up to `excerpt_separator`, rendered
    /// through Liquid and the converter but never placed in a layout.
    pub fn render_excerpt(
        &self,
        site: &Site,
        collection: &Collection,
        doc: &Document,
        payload_in: &Payload,
        state: &RenderState,
    ) -> Result<String> {
        // An explicit `excerpt:` in front matter is used verbatim.
        if let Some(v) = doc.data.get("excerpt").filter(|v| v.truthy()) {
            return Ok(v.to_string());
        }
        let separator = doc
            .data
            .get("excerpt_separator")
            .and_then(Value::as_str)
            .unwrap_or_else(|| site.config.str("excerpt_separator"))
            .to_string();
        if separator.is_empty() {
            return Ok(String::new());
        }

        let extracted = extract_excerpt(&doc.content, &separator);

        let mut payload = payload_in.root();
        let data = doc_to_liquid(site, collection, doc, state);
        payload.insert("page", LaxValue::Object(LaxObject::from_value_object(&data)));

        let mut output = extracted;
        if render_with_liquid(&doc.data, &output) {
            output = self.render_liquid(&output, &payload, &doc.relative_path)?;
        }
        if site.is_markdown(&doc.extname) {
            output = crate::markdown::convert(site, &output);
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
fn convert(site: &Site, page: &Page, content: &str) -> Result<String> {
    if site.is_markdown(&page.ext) {
        Ok(crate::markdown::convert(site, content))
    } else if crate::site::is_sass(&page.ext) {
        crate::sass::compile(site, content, page.ext == ".sass")
    } else {
        Ok(content.to_string())
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

/// `Document#to_liquid`, reading whatever render state the document has
/// reached so far.
pub fn doc_to_liquid(
    site: &Site,
    collection: &Collection,
    doc: &Document,
    state: &RenderState,
) -> Object {
    let url = site.doc_url(doc);
    let rendered = state.get(&doc.relative_path);
    let content = rendered.map(|r| r.content.as_str()).unwrap_or(&doc.content);
    let output = rendered.map(|r| r.output.as_str()).unwrap_or("");
    let excerpt = rendered.map(|r| r.excerpt.as_str()).unwrap_or("");
    document_to_liquid(doc, collection, &url, content, output, excerpt)
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
pub fn site_payload(site: &Site, state: &RenderState) -> LaxObject {
    let mut root = LaxObject::new();
    root.insert("site", LaxValue::Object(site_drop(site, state)));

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
fn site_drop(site: &Site, state: &RenderState) -> LaxObject {
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
    // Each document's Liquid object is built once and cloned into the several
    // places the drop exposes it.
    let mut built: HashMap<String, LaxValue> = HashMap::new();
    for (collection, doc) in site.documents() {
        built.insert(
            doc.relative_path.clone(),
            LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(
                site, collection, doc, state,
            ))),
        );
    }
    let get = |doc: &Document| built.get(&doc.relative_path).cloned().unwrap_or(LaxValue::Nil);

    // `SiteDrop#posts` is newest-first, the reverse of the stored order.
    let posts: Vec<LaxValue> = site
        .collections
        .get("posts")
        .map(|c| c.docs.iter().rev().map(&get).collect())
        .unwrap_or_default();
    drop.insert("posts", LaxValue::Array(posts));

    let documents: Vec<LaxValue> = site.documents().iter().map(|(_, d)| get(d)).collect();
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
                LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(
                    site, collection, d, state,
                )))
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
                            LaxValue::Object(LaxObject::from_value_object(&doc_to_liquid(site, c, d, state)))
                        })
                        .collect(),
                ),
            );
            LaxValue::Object(o)
        })
        .collect();
    drop.insert("collections", LaxValue::Array(collections));

    drop.insert("tags", LaxValue::Object(group_by(site, "tags", &get)));
    drop.insert("categories", LaxValue::Object(group_by(site, "categories", &get)));
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
fn group_by(site: &Site, key: &str, get: &dyn Fn(&Document) -> LaxValue) -> LaxObject {
    let mut groups: indexmap::IndexMap<String, Vec<LaxValue>> = indexmap::IndexMap::new();
    if let Some(collection) = site.collections.get("posts") {
        for doc in &collection.docs {
            for value in crate::document::string_list(doc.data.get(key)) {
                groups.entry(value).or_default().push(get(doc));
            }
        }
    }
    let mut out = LaxObject::new();
    for (k, mut v) in groups {
        // `Site#post_attr_hash` sorts each group then reverses it, so posts
        // within a tag or category are newest first.
        v.reverse();
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

/// `Excerpt#extract_excerpt`: everything before the separator, with any
/// Markdown link reference definitions the excerpt refers to appended so that
/// `[text][ref]` still resolves once the tail is gone.
fn extract_excerpt(content: &str, separator: &str) -> String {
    let (head, tail) = match content.find(separator) {
        None => return content.to_string(),
        Some(i) => (&content[..i], &content[i + separator.len()..]),
    };
    if tail.is_empty() {
        return head.to_string();
    }

    let re = regex::Regex::new(r"(?m)^ {0,3}(\[[^\]]+\])(:.+)$").unwrap();
    let definitions: Vec<String> = re
        .captures_iter(tail)
        .filter(|c| head.contains(&c[1]))
        .map(|c| format!("{}{}", &c[1], &c[2]))
        .collect();

    if definitions.is_empty() {
        return head.to_string();
    }
    format!("{}\n\n{}", head, definitions.join("\n"))
}
