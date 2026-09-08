//! `Jekyll::Utils.slugify` and `titleize_slug`.

use regex::Regex;
use std::sync::OnceLock;

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
