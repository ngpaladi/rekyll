//! Site generation: render everything, then write it to the destination.

use crate::render::{Payload, RenderState, Rendered, Renderer};
use crate::site::Site;
use anyhow::{Context, Result};
use std::path::Path;

pub fn build(source: &Path, dest: &Path) -> Result<()> {
    let mut site = Site::new(source, dest)?;
    site.read()?;

    let renderer = Renderer::new(&site)?;

    // Render before writing so a template error leaves the destination alone.
    let mut rendered: Vec<(std::path::PathBuf, String)> = Vec::new();

    // `Site#render` renders documents first, updating each one's converted
    // content and output as it goes, then renders pages against that state.
    let mut payload = Payload::new(&site);
    let mut state = RenderState::new();
    for (collection, doc) in site.documents() {
        let (content, output) = renderer
            .render_document(&site, collection, doc, &payload, &state)
            .with_context(|| format!("rendering {}", doc.relative_path))?;
        let excerpt = renderer
            .render_excerpt(&site, collection, doc, &payload, &state)
            .with_context(|| format!("rendering excerpt of {}", doc.relative_path))?;

        let done = Rendered { content, output: output.clone(), excerpt };
        payload.update_document(&doc.relative_path, &done);
        state.insert(doc.relative_path.clone(), done);

        if collection.write() {
            rendered.push((site.doc_destination(doc), output));
        }
    }

    for (i, page) in site.pages.iter().enumerate() {
        let (content, output) = renderer
            .render_page(&site, page, &payload)
            .with_context(|| format!("rendering {}", page.relative_path()))?;
        payload.update_page(
            i,
            &Rendered { content, output: output.clone(), excerpt: String::new() },
        );
        rendered.push((site.page_destination(page), output));
    }

    clean_destination(&site)?;

    for (path, output) in rendered {
        make_parent(&path)?;
        std::fs::write(&path, output).with_context(|| format!("writing {}", path.display()))?;
    }
    for file in &site.static_files {
        let target = site.static_file_destination(file);
        make_parent(&target)?;
        std::fs::copy(&file.source, &target)
            .with_context(|| format!("copying {}", file.source.display()))?;
    }
    Ok(())
}

fn make_parent(path: &Path) -> Result<()> {
    match path.parent() {
        Some(parent) => Ok(std::fs::create_dir_all(parent)?),
        None => Ok(()),
    }
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
