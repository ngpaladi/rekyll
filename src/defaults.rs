//! Front-matter defaults (`jekyll/frontmatter_defaults.rb`).
//!
//! `defaults:` in `_config.yml` is a list of `{scope: {path, type}, values: {}}`
//! sets. Every matching set contributes, with more specific scopes winning.

use crate::config::deep_merge;
use crate::value::{Object, Value};

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

    fn applies(&self, set: &Set, path: &str, doc_type: &str) -> bool {
        self.applies_type(set, doc_type) && self.applies_path(set, path)
    }

    fn applies_type(&self, set: &Set, doc_type: &str) -> bool {
        match &set.doc_type {
            None => true,
            Some(t) => t == doc_type,
        }
    }

    fn applies_path(&self, set: &Set, path: &str) -> bool {
        if set.path.is_empty() {
            return true;
        }
        let sanitized = sanitize_path(path);
        if set.path.contains('*') {
            return glob_match(&set.path, &sanitized);
        }
        sanitized.starts_with(&self.strip_collections_dir(&sanitize_path(&set.path)))
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
