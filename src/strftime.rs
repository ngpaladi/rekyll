//! Ruby's `Time#strftime`.
//!
//! chrono's formatter is not a drop-in substitute: it has no `%^` upcase flag,
//! handles `%-`/`%_` padding differently, and Jekyll's permalink drop and
//! `date:` filter lean on both. This implements the directive set Ruby
//! supports, with its flag grammar (`-`, `_`, `0`, `^`, `#`, width).

use chrono::{DateTime, Datelike, FixedOffset, Timelike};

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];
const DAYS: [&str; 7] =
    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

#[derive(Default, Clone, Copy)]
struct Flags {
    no_pad: bool,
    space_pad: bool,
    zero_pad: bool,
    upcase: bool,
    swapcase: bool,
    width: Option<usize>,
}

pub fn strftime(t: &DateTime<FixedOffset>, format: &str) -> String {
    strftime_zoned(t, format, None)
}

/// As `strftime`, but with the zone abbreviation `%Z` should report.
///
/// A Ruby `Time` built from an explicit numeric offset has no zone name and
/// prints nothing for `%Z`; one produced by `Time#localtime` carries the
/// abbreviation of the process timezone. Jekyll's dates go through
/// `Utils.parse_date`, which calls `localtime`, so they do have one.
pub fn strftime_zoned(t: &DateTime<FixedOffset>, format: &str, zone: Option<&str>) -> String {
    let mut out = String::with_capacity(format.len() + 16);
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= chars.len() {
            out.push('%');
            break;
        }

        let mut flags = Flags::default();
        // Flag characters, then an optional width.
        loop {
            match chars.get(i) {
                Some('-') => flags.no_pad = true,
                Some('_') => flags.space_pad = true,
                Some('0') => flags.zero_pad = true,
                Some('^') => flags.upcase = true,
                Some('#') => flags.swapcase = true,
                _ => break,
            }
            i += 1;
        }
        let mut width = String::new();
        while let Some(c) = chars.get(i) {
            if c.is_ascii_digit() {
                width.push(*c);
                i += 1;
            } else {
                break;
            }
        }
        if !width.is_empty() {
            flags.width = width.parse().ok();
        }

        let directive = match chars.get(i) {
            Some(c) => *c,
            None => {
                out.push('%');
                break;
            }
        };
        i += 1;

        // `%:z` and `%::z` are colon-separated offset forms.
        if directive == ':' {
            let mut colons = 1;
            while chars.get(i) == Some(&':') {
                colons += 1;
                i += 1;
            }
            if chars.get(i) == Some(&'z') {
                i += 1;
                out.push_str(&offset(t, colons));
                continue;
            }
            out.push('%');
            for _ in 0..colons {
                out.push(':');
            }
            continue;
        }

        out.push_str(&expand(t, directive, flags, zone));
    }
    out
}

fn expand(t: &DateTime<FixedOffset>, directive: char, flags: Flags, zone: Option<&str>) -> String {
    let num = |v: i64, default_width: usize| pad_num(v, default_width, flags, '0');
    let text = |s: &str| case(s, flags);

    match directive {
        'Y' => pad_num(t.year() as i64, 4, flags, '0'),
        'C' => num((t.year() / 100) as i64, 2),
        'y' => num((t.year().rem_euclid(100)) as i64, 2),
        'm' => num(t.month() as i64, 2),
        'd' => num(t.day() as i64, 2),
        // %e is day-of-month, space padded by default.
        'e' => pad_num(t.day() as i64, 2, flags, ' '),
        'j' => num(t.ordinal() as i64, 3),
        'H' => num(t.hour() as i64, 2),
        'k' => pad_num(t.hour() as i64, 2, flags, ' '),
        'I' => num(hour12(t) as i64, 2),
        'l' => pad_num(hour12(t) as i64, 2, flags, ' '),
        'M' => num(t.minute() as i64, 2),
        'S' => num(t.second() as i64, 2),
        'L' => num((t.timestamp_subsec_millis()) as i64, 3),
        'N' => num((t.timestamp_subsec_nanos()) as i64, 9),
        's' => num(t.timestamp(), 1),
        'z' => offset(t, 0),
        'Z' => text(zone.unwrap_or("")),
        'a' => text(&DAYS[t.weekday().num_days_from_sunday() as usize][..3]),
        'A' => text(DAYS[t.weekday().num_days_from_sunday() as usize]),
        'b' | 'h' => text(&MONTHS[(t.month() - 1) as usize][..3]),
        'B' => text(MONTHS[(t.month() - 1) as usize]),
        'p' => text(if t.hour() < 12 { "AM" } else { "PM" }),
        'P' => text(if t.hour() < 12 { "am" } else { "pm" }),
        'u' => num(t.weekday().number_from_monday() as i64, 1),
        'w' => num(t.weekday().num_days_from_sunday() as i64, 1),
        'G' => pad_num(t.iso_week().year() as i64, 4, flags, '0'),
        'g' => num((t.iso_week().year().rem_euclid(100)) as i64, 2),
        'V' => num(t.iso_week().week() as i64, 2),
        'U' => num(week_of_year(t, 0) as i64, 2),
        'W' => num(week_of_year(t, 1) as i64, 2),
        'D' | 'x' => strftime_zoned(t, "%m/%d/%y", zone),
        'F' => strftime_zoned(t, "%Y-%m-%d", zone),
        'T' | 'X' => strftime_zoned(t, "%H:%M:%S", zone),
        'R' => strftime_zoned(t, "%H:%M", zone),
        'r' => strftime_zoned(t, "%I:%M:%S %p", zone),
        'c' => strftime_zoned(t, "%a %b %e %H:%M:%S %Y", zone),
        '+' => strftime_zoned(t, "%a %b %e %H:%M:%S %Z %Y", zone),
        'n' => "\n".to_string(),
        't' => "\t".to_string(),
        '%' => "%".to_string(),
        other => format!("%{other}"),
    }
}

fn hour12(t: &DateTime<FixedOffset>) -> u32 {
    match t.hour() % 12 {
        0 => 12,
        h => h,
    }
}

/// Ruby's `%U`/`%W`: week of year counting from the first Sunday (`%U`) or
/// first Monday (`%W`); days before that are week 00.
fn week_of_year(t: &DateTime<FixedOffset>, start_monday: u32) -> u32 {
    let yday = t.ordinal() as i32; // 1-based
    let wday = t.weekday().num_days_from_sunday() as i32;
    let shifted = if start_monday == 1 { (wday + 6) % 7 } else { wday };
    (((yday - 1) - shifted + 7) / 7) as u32
}

fn offset(t: &DateTime<FixedOffset>, colons: usize) -> String {
    let total = t.offset().local_minus_utc();
    let sign = if total < 0 { '-' } else { '+' };
    let total = total.abs();
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    match colons {
        0 => format!("{sign}{h:02}{m:02}"),
        1 => format!("{sign}{h:02}:{m:02}"),
        _ => format!("{sign}{h:02}:{m:02}:{s:02}"),
    }
}

fn pad_num(v: i64, default_width: usize, flags: Flags, default_pad: char) -> String {
    if flags.no_pad {
        return v.to_string();
    }
    let width = flags.width.unwrap_or(default_width);
    let pad = if flags.zero_pad {
        '0'
    } else if flags.space_pad {
        ' '
    } else {
        default_pad
    };

    let negative = v < 0;
    let digits = v.abs().to_string();
    let target = if negative { width.saturating_sub(1) } else { width };
    let padded = if digits.len() >= target {
        digits
    } else {
        let mut s = String::new();
        for _ in 0..(target - digits.len()) {
            s.push(pad);
        }
        s.push_str(&digits);
        s
    };
    if negative {
        format!("-{padded}")
    } else {
        padded
    }
}

fn case(s: &str, flags: Flags) -> String {
    let s = if let Some(w) = flags.width {
        if s.len() < w {
            format!("{}{}", " ".repeat(w - s.len()), s)
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    };
    if flags.upcase {
        s.to_uppercase()
    } else if flags.swapcase {
        // Ruby's "change case" flag works on the whole field, not per
        // character: "Jan" becomes "JAN" and "AM" becomes "am".
        if s.chars().any(char::is_lowercase) {
            s.to_uppercase()
        } else {
            s.to_lowercase()
        }
    } else {
        s
    }
}
