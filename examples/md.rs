//! Renders corpus files through rekyll's converter in the same shape as
//! tests/harness/kramdown_ref.rb, so the two can be diffed.

fn main() {
    let site = rekyll::site::Site::new(
        std::path::Path::new("tests/fixtures/01-static"),
        std::path::Path::new("/tmp/rekyll-diff/unused"),
    )
    .expect("site");
    for path in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&path).expect("read");
        let name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy();
        println!("===== {name} =====");
        print!("{}", rekyll::markdown::convert(&site, &src));
    }
}
