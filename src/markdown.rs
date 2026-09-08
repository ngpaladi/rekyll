//! Markdown conversion. Currently a placeholder so the pipeline is wired;
//! the Kramdown-compatible emitter lands in a later stage.

use crate::site::Site;

pub fn convert(_site: &Site, content: &str) -> String {
    content.to_string()
}
