//! Site configuration: defaults, merge order, the derived collection set,
//! and front-matter defaults.
//!
//! Ported from `jekyll/configuration.rb` and `frontmatter_defaults.rb`. The defaults are reproduced verbatim
//! because they leak into rendered output through `site.*`.

use crate::value::{Object, Value};
use anyhow::Result;
use std::path::Path;

/// `Jekyll::Configuration::DEFAULTS`, expressed as YAML so the scalar types
/// resolve through exactly the same path as a user's `_config.yml`.
const DEFAULTS_YAML: &str = r#"
collections_dir: ""
cache_dir: ".jekyll-cache"
plugins_dir: "_plugins"
layouts_dir: "_layouts"
data_dir: "_data"
includes_dir: "_includes"
collections: {}

safe: false
include: [".htaccess"]
exclude: []
keep_files: [".git", ".svn"]
encoding: "utf-8"
markdown_ext: "markdown,mkdown,mkdn,mkd,md"
strict_front_matter: false

show_drafts: ~
limit_posts: 0
future: false
unpublished: false

whitelist: []
plugins: []

markdown: "kramdown"
highlighter: "rouge"
lsi: false
excerpt_separator: "\n\n"
incremental: false

detach: false
port: "4000"
host: "127.0.0.1"
baseurl: ~
show_dir_listing: false

permalink: "date"
paginate_path: "/page:num"
timezone: ~

quiet: false
verbose: false
defaults: []

liquid:
  error_mode: "warn"
  strict_filters: false
  strict_variables: false

kramdown:
  auto_ids: true
  toc_levels: [1, 2, 3, 4, 5, 6]
  entity_output: "as_char"
  smart_quotes: "lsquo,rsquo,ldquo,rdquo"
  input: "GFM"
  hard_wrap: false
  guess_lang: true
  footnote_nr: 1
  show_warnings: false
"#;

/// `Configuration::DEFAULT_EXCLUDES`.
const DEFAULT_EXCLUDES: &[&str] = &[
    ".sass-cache",
    ".jekyll-cache",
    "gemfiles",
    "Gemfile",
    "Gemfile.lock",
    "node_modules",
    "vendor/bundle/",
    "vendor/cache/",
    "vendor/gems/",
    "vendor/ruby/",
];

/// `STYLE_TO_PERMALINK`.
fn style_to_permalink(style: &str) -> String {
    match style {
        "none" => "/:categories/:title:output_ext",
        "date" => "/:categories/:year/:month/:day/:title:output_ext",
        "ordinal" => "/:categories/:year/:y_day/:title:output_ext",
        "pretty" => "/:categories/:year/:month/:day/:title/",
        "weekdate" => "/:categories/:year/W:week/:short_day/:title:output_ext",
        other => other,
    }
    .to_string()
}

#[derive(Debug, Clone)]
pub struct Config(pub Object);

impl Config {
    pub fn defaults() -> Config {
        let v = crate::yaml::load(DEFAULTS_YAML).expect("built-in defaults must parse");
        Config(v.as_object().cloned().unwrap_or_default())
    }

    /// Build the effective configuration for a source directory.
    pub fn load(source: &Path, destination: &Path) -> Result<Config> {
        let mut cfg = Config::defaults();

        // Jekyll probes _config.yml, then .yaml, then .toml. We support YAML.
        let file = ["_config.yml", "_config.yaml"]
            .iter()
            .map(|f| source.join(f))
            .find(|p| p.exists());

        if let Some(path) = file {
            let text = std::fs::read_to_string(&path)?;
            let user = crate::yaml::load(&text)?;
            if let Some(user) = user.as_object() {
                cfg.0 = deep_merge(&cfg.0, user);
            }
        }

        cfg.0.insert("source".into(), Value::str(source.to_string_lossy()));
        cfg.0.insert("destination".into(), Value::str(destination.to_string_lossy()));

        cfg.add_default_collections();
        cfg.add_default_excludes();
        Ok(cfg)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn str(&self, key: &str) -> &str {
        self.0.get(key).and_then(Value::as_str).unwrap_or("")
    }

    pub fn bool(&self, key: &str) -> bool {
        // Jekyll tests these with Ruby truthiness, so a missing key is false
        // but a present-and-non-nil value is true.
        self.0.get(key).map(Value::truthy).unwrap_or(false)
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.0
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|v| v.to_string()).collect())
            .unwrap_or_default()
    }

    /// `add_default_collections`: guarantee a `posts` collection that outputs,
    /// and give it the permalink implied by the top-level `permalink` style.
    fn add_default_collections(&mut self) {
        if matches!(self.0.get("collections"), None | Some(Value::Null)) {
            return;
        }

        // A list of names is sugar for a map of empty option hashes.
        if let Some(Value::Array(names)) = self.0.get("collections").cloned() {
            let mut o = Object::new();
            for n in names {
                o.insert(n.to_string(), Value::Object(Object::new()));
            }
            self.0.insert("collections".into(), Value::Object(o));
        }

        let permalink = self.0.get("permalink").cloned();
        let mut collections = self
            .0
            .get("collections")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        // `deep_merge_hashes({"posts" => {}}, collections)` puts posts first
        // whether or not the user listed it, and listing it later does not
        // move it.
        let mut ordered = Object::new();
        ordered.insert(
            "posts".into(),
            collections
                .shift_remove("posts")
                .unwrap_or_else(|| Value::Object(Object::new())),
        );
        ordered.extend(collections);
        collections = ordered;

        let posts = collections
            .entry("posts".into())
            .or_insert_with(|| Value::Object(Object::new()));
        if let Some(posts) = posts.as_object_mut() {
            posts.insert("output".into(), Value::Bool(true));
            if let Some(p) = permalink.filter(Value::truthy) {
                posts
                    .entry("permalink".into())
                    .or_insert_with(|| Value::str(style_to_permalink(&p.to_string())));
            }
        }

        self.0.insert("collections".into(), Value::Object(collections));
    }

    /// `add_default_excludes`: append the built-in excludes, then `uniq!`.
    fn add_default_excludes(&mut self) {
        if matches!(self.0.get("exclude"), None | Some(Value::Null)) {
            return;
        }
        let mut list = self.list("exclude");
        for d in DEFAULT_EXCLUDES {
            list.push((*d).to_string());
        }
        let mut seen = std::collections::HashSet::new();
        list.retain(|s| seen.insert(s.clone()));
        self.0
            .insert("exclude".into(), Value::Array(list.into_iter().map(Value::Str).collect()));
    }
}

/// `Jekyll::Utils.deep_merge_hashes`.
///
/// Nested hashes merge recursively; a `nil` on the right keeps the left value,
/// which is why `key:` with no value in `_config.yml` does not clear a default.
pub fn deep_merge(base: &Object, overlay: &Object) -> Object {
    let mut out = base.clone();
    for (k, new) in overlay {
        match (out.get(k), new) {
            (_, Value::Null) if out.contains_key(k) => {}
            (Some(Value::Object(old)), Value::Object(new)) => {
                out.insert(k.clone(), Value::Object(deep_merge(old, new)));
            }
            _ => {
                out.insert(k.clone(), new.clone());
            }
        }
    }
    out
}

// --- Front-matter defaults (`jekyll/frontmatter_defaults.rb`) ---
//
// `defaults:` in `_config.yml` is a list of `{scope: {path, type}, values: {}}`
// sets. Every matching set contributes, with more specific scopes winning.

pub struct Defaults {
    sets: Vec<Set>,
    collections_dir: String,
}

struct Set {
    path: String,
    doc_type: Option<String>,
    values: Object,
}

impl Defaults {
    pub fn new(config_defaults: Option<&Value>, collections_dir: &str) -> Defaults {
        let mut sets = Vec::new();
        if let Some(Value::Array(list)) = config_defaults {
            for entry in list {
                // `valid?`: a set needs a "values" hash; "scope" is optional.
                let values = match entry.get("values").and_then(Value::as_object) {
                    Some(v) => v.clone(),
                    None => continue,
                };
                let scope = entry.get("scope");
                let path = scope
                    .and_then(|s| s.get("path"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let doc_type = scope
                    .and_then(|s| s.get("type"))
                    .and_then(Value::as_str)
                    // Jekyll rewrites the deprecated singular type names.
                    .map(|t| match t {
                        "page" => "pages".to_string(),
                        "post" => "posts".to_string(),
                        "draft" => "drafts".to_string(),
                        other => other.to_string(),
                    });
                sets.push(Set { path, doc_type, values });
            }
        }
        Defaults { sets, collections_dir: collections_dir.to_string() }
    }

    /// `FrontmatterDefaults#all`: merge every matching set, ordering by scope
    /// precedence so a more specific path overrides a broader one.
    pub fn all(&self, path: &str, doc_type: &str) -> Object {
        let mut defaults = Object::new();
        let mut old_scope: Option<&Set> = None;

        for set in self.sets.iter().filter(|s| self.applies(s, path, doc_type)) {
            if has_precedence(old_scope, set) {
                defaults = deep_merge(&defaults, &set.values);
                old_scope = Some(set);
            } else {
                // A weaker scope only fills gaps the stronger one left.
                defaults = deep_merge(&set.values, &defaults);
            }
        }
        defaults
    }

    /// `applies_type?` and `applies_path?`: an untyped scope matches every
    /// type; an empty path matches everything, a glob is matched, and a plain
    /// path is a prefix.
    fn applies(&self, set: &Set, path: &str, doc_type: &str) -> bool {
        if set.doc_type.as_deref().is_some_and(|t| t != doc_type) {
            return false;
        }
        let sanitized = sanitize_path(path);
        set.path.is_empty()
            || if set.path.contains('*') {
                glob_match(&set.path, &sanitized)
            } else {
                sanitized.starts_with(&self.strip_collections_dir(&sanitize_path(&set.path)))
            }
    }

    fn strip_collections_dir(&self, path: &str) -> String {
        if self.collections_dir.is_empty() {
            return path.to_string();
        }
        let prefix = format!("{}/", self.collections_dir);
        path.strip_prefix(&prefix).unwrap_or(path).to_string()
    }
}

/// `has_precedence?`: a longer scope path wins; on a tie, a typed scope wins.
fn has_precedence(old: Option<&Set>, new: &Set) -> bool {
    let old = match old {
        None => return true,
        Some(o) => o,
    };
    let new_path = sanitize_path(&new.path);
    let old_path = sanitize_path(&old.path);
    if new_path.len() != old_path.len() {
        new_path.len() >= old_path.len()
    } else if new.doc_type.is_some() {
        true
    } else {
        old.doc_type.is_none()
    }
}

/// `sanitize_path`: defaults scopes are written without a leading slash.
fn sanitize_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

fn glob_match(pattern: &str, path: &str) -> bool {
    globset::GlobBuilder::new(pattern)
        .literal_separator(false)
        .build()
        .map(|g| g.compile_matcher().is_match(path))
        .unwrap_or(false)
        // Jekyll globs the scope and then prefix-matches, so a pattern naming a
        // directory also covers everything beneath it.
        || path.starts_with(pattern.trim_end_matches(['*', '/']))
}
