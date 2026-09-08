//! Mirrors tests/harness/strftime_ref.rb so the two outputs can be diffed.

use chrono::{FixedOffset, TimeZone};

fn main() {
    let path = std::env::args().nth(1).expect("usage: strftime_dump <file>");
    let text = std::fs::read_to_string(path).expect("read formats");
    let times = [
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2020, 1, 2, 3, 4, 5).unwrap(),
        FixedOffset::east_opt(-5 * 3600).unwrap().with_ymd_and_hms(2021, 12, 31, 23, 59, 59).unwrap(),
        FixedOffset::east_opt(19800).unwrap().with_ymd_and_hms(2024, 2, 29, 12, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(1999, 7, 4, 0, 0, 0).unwrap(),
    ];
    for fmt in text.lines() {
        for (i, t) in times.iter().enumerate() {
            println!("{i}\t{fmt}\t{}", rekyll::strftime::strftime(t, fmt));
        }
    }
}
