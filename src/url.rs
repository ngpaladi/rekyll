//! URLs: permalink expansion and escaping (`jekyll/url.rb`), the
//! `relative_url`/`absolute_url` filters (`Jekyll::Filters::URLFilters`), and
//! `Utils.slugify`.

use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use std::sync::OnceLock;

/// Characters Addressable leaves unescaped in a path: unreserved, sub-delims,
/// ":", "@" and "/". Everything outside that set is percent-encoded. Jekyll
/// then escapes "#" separately because Addressable treats it as a fragment.
const PATH_SAFE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'%');

/// `URL.escape_path`.
pub fn escape_path(path: &str) -> String {
    static SIMPLE: OnceLock<Regex> = OnceLock::new();
    let simple = SIMPLE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9./-]+$").unwrap());
    if path.is_empty() || simple.is_match(path) {
        return path.to_string();
    }
    utf8_percent_encode(path, PATH_SAFE).to_string().replace('#', "%23")
}

/// `URL.unescape_path`.
pub fn unescape_path(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// `URL#sanitize_url`: force a leading slash, neutralise "..", drop "./",
/// and squeeze runs of slashes down to one.
pub fn sanitize_url(s: &str) -> String {
    let mut result = format!("/{s}").replace("..", "/");
    result = result.replace("./", "");
    squeeze_slashes(&result)
}

fn squeeze_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

/// `URL#generate_url_from_hash`: substitute `:key` placeholders in order,
/// stopping early once no colon remains. A `nil` value removes the preceding
/// slash as well, so absent categories do not leave a double slash behind.
pub fn generate_url(template: &str, placeholders: &[(&str, Option<String>)]) -> String {
    let mut result = template.to_string();
    for (key, value) in placeholders {
        if !result.contains(':') {
            break;
        }
        match value {
            None => result = result.replace(&format!("/:{key}"), ""),
            Some(v) => result = result.replace(&format!(":{key}"), &escape_path(v)),
        }
    }
    result
}

/// `Utils.add_permalink_suffix`.
pub fn add_permalink_suffix(template: &str, permalink_style: &str) -> String {
    let mut t = template.to_string();
    match permalink_style {
        "pretty" => t.push('/'),
        "date" | "ordinal" | "none" => t.push_str(":output_ext"),
        other => {
            if other.ends_with('/') {
                t.push('/');
            }
            if other.ends_with(":output_ext") {
                t.push_str(":output_ext");
            }
        }
    }
    t
}

// --- URLFilters ---

/// `relative_url`: prepend the site's baseurl unless the input is already
/// absolute (has a scheme and authority).
pub fn relative_url(input: &str, baseurl: &str) -> String {
    if is_absolute(input) {
        return input.to_string();
    }
    let base = baseurl.trim_end_matches('/');
    normalize(&format!("{}{}", ensure_leading_slash(base), ensure_leading_slash(input)))
}

/// `absolute_url`: site.url followed by the relative URL.
pub fn absolute_url(input: &str, site_url: &str, baseurl: &str) -> String {
    if is_absolute(input) {
        return input.to_string();
    }
    if site_url.is_empty() {
        return relative_url(input, baseurl);
    }
    normalize(&format!("{}{}", site_url, relative_url(input, baseurl)))
}

/// `strip_index`.
pub fn strip_index(input: &str) -> String {
    if let Some(stripped) = input.strip_suffix("/index.html") {
        return format!("{stripped}/");
    }
    if let Some(stripped) = input.strip_suffix("/index.htm") {
        return format!("{stripped}/");
    }
    input.to_string()
}

fn ensure_leading_slash(input: &str) -> String {
    if input.is_empty() || input.starts_with('/') {
        input.to_string()
    } else {
        format!("/{input}")
    }
}

/// A URI is absolute for Addressable's purposes when it has a scheme.
fn is_absolute(input: &str) -> bool {
    match input.find(':') {
        None => false,
        Some(i) => {
            let scheme = &input[..i];
            !scheme.is_empty()
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        }
    }
}

/// `Addressable::URI#normalize` as it affects these paths: resolve "." and
/// ".." segments. Percent-encoding is left alone since Jekyll's URLs are
/// already escaped by `URL.escape_path`.
fn normalize(url: &str) -> String {
    let (prefix, path) = match url.find("://") {
        Some(i) => match url[i + 3..].find('/') {
            Some(j) => url.split_at(i + 3 + j),
            None => return url.to_string(),
        },
        None => ("", url),
    };

    let trailing_slash = path.ends_with('/') && path.len() > 1;
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let mut joined = format!("/{}", out.join("/"));
    if trailing_slash && !joined.ends_with('/') {
        joined.push('/');
    }
    format!("{prefix}{joined}")
}

// --- Utils.slugify ---


fn mode_regex(mode: &str) -> Option<&'static Regex> {
    static RAW: OnceLock<Regex> = OnceLock::new();
    static DEFAULT: OnceLock<Regex> = OnceLock::new();
    static PRETTY: OnceLock<Regex> = OnceLock::new();
    static ASCII: OnceLock<Regex> = OnceLock::new();
    match mode {
        "raw" => Some(RAW.get_or_init(|| Regex::new(r"\s+").unwrap())),
        "pretty" => Some(
            PRETTY.get_or_init(|| Regex::new(r"[^\p{M}\p{L}\p{Nd}._~!$&'()+,;=@]+").unwrap()),
        ),
        "ascii" => Some(ASCII.get_or_init(|| Regex::new(r"[^A-Za-z0-9]+").unwrap())),
        // "latin" transliterates first, then falls through to the default set.
        "default" | "latin" => {
            Some(DEFAULT.get_or_init(|| Regex::new(r"[^\p{M}\p{L}\p{Nd}]+").unwrap()))
        }
        _ => None,
    }
}

/// `Utils.slugify`. An unrecognised mode returns the string unchanged apart
/// from case, which is how Jekyll treats `slugify: none`.
pub fn slugify(string: &str, mode: &str, cased: bool) -> String {
    let re = match mode_regex(mode) {
        Some(r) => r,
        None => return if cased { string.to_string() } else { string.to_lowercase() },
    };

    let slug = re.replace_all(string, "-");
    let slug = slug.trim_matches('-').to_string();
    if cased {
        slug
    } else {
        slug.to_lowercase()
    }
}

/// `Utils.titleize_slug`: "hello-world" becomes "Hello World".
pub fn titleize_slug(slug: &str) -> String {
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                // Ruby's String#capitalize also downcases the remainder.
                Some(f) => {
                    f.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
