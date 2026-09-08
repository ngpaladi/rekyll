//! Times the phases of a build to locate hot spots.
use std::time::Instant;

fn main() {
    let src = std::env::args().nth(1).expect("usage: profile <source>");
    let t0 = Instant::now();
    let mut site = rekyll::site::Site::new(
        std::path::Path::new(&src),
        std::path::Path::new("/tmp/rekyll-diff/profile-out"),
    )
    .unwrap();
    site.read().unwrap();
    println!("read:            {:?}", t0.elapsed());

    let t1 = Instant::now();
    let renderer = rekyll::render::Renderer::new(&site).unwrap();
    println!("parser build:    {:?}", t1.elapsed());

    let t2 = Instant::now();
    let mut payload = rekyll::render::Payload::new(&site);
    println!("payload build:   {:?}", t2.elapsed());

    let t3 = Instant::now();
    for (_, doc) in site.documents() {
        std::hint::black_box(site.doc_url(doc));
    }
    println!("all doc_urls:    {:?}", t3.elapsed());

    let t4 = Instant::now();
    let mut st = rekyll::render::RenderState::new();
    for (collection, doc) in site.documents() {
        let (c, o) = renderer
            .render_document(&site, collection, doc, &payload, &st)
            .unwrap();
        let done = rekyll::render::Rendered { content: c, output: o, excerpt: String::new() };
        payload.update_document(&doc.relative_path, &done);
        st.insert(doc.relative_path.clone(), done);
    }
    println!("render all docs: {:?}", t4.elapsed());

    let t5 = Instant::now();
    for page in &site.pages {
        std::hint::black_box(renderer.render_page(&site, page, &payload).unwrap());
    }
    println!("render pages:    {:?}", t5.elapsed());
}
