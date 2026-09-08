//! Liquid rendering, payload assembly and the layout chain (`renderer.rb`).

use crate::config::deep_merge;
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
    pub fn new() -> Result<Renderer> {
        let parser = liquid::ParserBuilder::with_stdlib()
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
    drop.insert("posts", LaxValue::Array(Vec::new()));
    drop.insert("documents", LaxValue::Array(Vec::new()));
    drop.insert("collections", LaxValue::Array(Vec::new()));
    drop.insert("tags", LaxValue::Object(LaxObject::new()));
    drop.insert("categories", LaxValue::Object(LaxObject::new()));
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
