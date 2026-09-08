//! Site generation: render everything, then write it to the destination.

use crate::render::Renderer;
use crate::site::Site;
use anyhow::{Context, Result};
use std::path::Path;

pub fn build(source: &Path, dest: &Path) -> Result<()> {
    let mut site = Site::new(source, dest)?;
    site.read()?;

    let renderer = Renderer::new(&site)?;

    // Render before writing so a template error leaves the destination alone.
    let mut rendered: Vec<(std::path::PathBuf, String)> = Vec::new();
    for page in &site.pages {
        let output = renderer
            .render_page(&site, page)
            .with_context(|| format!("rendering {}", page.relative_path()))?;
        rendered.push((site.page_destination(page), output));
    }

    for (collection, doc) in site.documents() {
        if !collection.write() {
            continue;
        }
        let output = renderer
            .render_document(&site, collection, doc)
            .with_context(|| format!("rendering {}", doc.relative_path))?;
        rendered.push((site.doc_destination(doc), output));
    }

    clean_destination(&site)?;

    for (path, output) in rendered {
        write_file(&path, output.as_bytes())?;
    }
    for file in &site.static_files {
        let target = site.dest.join(file.relative_path());
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&file.source, &target)
            .with_context(|| format!("copying {}", file.source.display()))?;
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// `Jekyll::Cleaner`: remove everything in the destination except `keep_files`.
fn clean_destination(site: &Site) -> Result<()> {
    if !site.dest.exists() {
        std::fs::create_dir_all(&site.dest)?;
        return Ok(());
    }
    let keep = site.config.list("keep_files");
    for entry in std::fs::read_dir(&site.dest)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if keep.contains(&name) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}
