//! Ruby `Time` values in the site's timezone.
//!
//! Jekyll normalises every document date through
//! `Utils.parse_date` -> `Time.parse(input).localtime`, which resolves the
//! string in (or converts it to) the process timezone. Jekyll sets that
//! timezone from `timezone:` in `_config.yml`, so dates — and therefore
//! date-based permalinks — depend on it.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike};
use chrono_tz::{OffsetName, Tz};
use regex::Regex;
use std::sync::OnceLock;

/// A `Time` together with the zone abbreviation `%Z` should print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RTime {
    pub at: DateTime<FixedOffset>,
    pub zone: Option<String>,
}

impl RTime {
    /// Ruby's `Time#strftime` (the `strftime-ruby` crate reproduces its
    /// flags: `%-d`, `%^b`, `%_H`, widths). Ruby raises on a bad format and
    /// Liquid does not rescue it, so a template's `date:` argument fails the
    /// build there too; that is `try_format`. Plain `format` is for the
    /// constant formats in rekyll itself.
    pub fn try_format(&self, fmt: &str) -> Result<String> {
        strftime::string::strftime(self, fmt).map_err(|e| anyhow!("invalid date format {fmt:?}: {e}"))
    }

    pub fn format(&self, fmt: &str) -> String {
        self.try_format(fmt).expect("constant format")
    }

    /// Ruby's `Time#to_s`.
    pub fn to_s(&self) -> String {
        self.format("%Y-%m-%d %H:%M:%S %z")
    }

    pub fn timestamp(&self) -> i64 {
        self.at.timestamp()
    }

    /// Day of month, for the ordinal form of `date_to_string`.
    pub fn at_day(&self) -> u32 {
        use chrono::Datelike;
        self.at.day()
    }
}

/// Resolve the site's timezone. An unknown name falls back to UTC, which is
/// also what a container with no zone database gives Jekyll.
pub fn site_timezone(name: Option<&str>) -> Tz {
    match name {
        Some(n) if !n.is_empty() => n.parse::<Tz>().unwrap_or(chrono_tz::UTC),
        _ => chrono_tz::UTC,
    }
}

fn date_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?x)
            (\d{4})-(\d{1,2})-(\d{1,2})
            (?:
                [\ Tt]+
                (\d{1,2}):(\d{2})
                (?::(\d{2}))?
                (?:\.(\d+))?
                \s*
                (Z|z|[-+]\d{1,2}:?\d{2}|[-+]\d{1,2})?
            )?",
        )
        .unwrap()
    })
}

/// `Utils.parse_date`: parse in, or convert to, the site timezone.
pub fn parse_date(input: &str, tz: Tz) -> Result<RTime> {
    let caps = date_regex()
        .captures(input)
        .ok_or_else(|| anyhow!("Invalid date '{input}'"))?;

    let y: i32 = caps[1].parse()?;
    let mo: u32 = caps[2].parse()?;
    let d: u32 = caps[3].parse()?;
    let h: u32 = caps.get(4).map_or(Ok(0), |m| m.as_str().parse())?;
    let mi: u32 = caps.get(5).map_or(Ok(0), |m| m.as_str().parse())?;
    let sec: u32 = caps.get(6).map_or(Ok(0), |m| m.as_str().parse())?;
    let nanos: u32 = caps
        .get(7)
        .map(|m| {
            let frac = format!("0.{}", m.as_str());
            (frac.parse::<f64>().unwrap_or(0.0) * 1e9).round() as u32
        })
        .unwrap_or(0);

    let date = NaiveDate::from_ymd_opt(y, mo, d).ok_or_else(|| anyhow!("Invalid date '{input}'"))?;
    let naive = date
        .and_hms_nano_opt(h, mi, sec, nanos)
        .ok_or_else(|| anyhow!("Invalid time in '{input}'"))?;

    let utc_instant = match caps.get(8).map(|m| m.as_str()) {
        // No zone in the string: the wall-clock time is local to the site.
        None => local_to_utc(naive, tz)?,
        Some("Z") | Some("z") => naive,
        Some(z) => naive - chrono::Duration::seconds(parse_offset(z)? as i64),
    };

    Ok(in_zone(utc_instant, tz))
}

/// Render a UTC instant as a `Time` in the given zone, carrying the
/// abbreviation `%Z` reports.
pub fn in_zone(utc_naive: NaiveDateTime, tz: Tz) -> RTime {
    let utc = chrono::Utc.from_utc_datetime(&utc_naive);
    let local = utc.with_timezone(&tz);
    let offset = local.offset().fix();
    RTime {
        at: offset.from_utc_datetime(&utc_naive),
        zone: Some(local.offset().abbreviation().unwrap_or("UTC").to_string()),
    }
}

/// Interpret a wall-clock time as local to `tz`, resolving DST gaps the way
/// Ruby does by taking the earlier of two candidate instants.
fn local_to_utc(naive: NaiveDateTime, tz: Tz) -> Result<NaiveDateTime> {
    use chrono::LocalResult;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(t) => Ok(t.naive_utc()),
        LocalResult::Ambiguous(a, _) => Ok(a.naive_utc()),
        LocalResult::None => {
            // The wall-clock time does not exist (spring-forward gap); shift
            // forward an hour to land on a real instant.
            let shifted = naive + chrono::Duration::hours(1);
            match tz.from_local_datetime(&shifted) {
                LocalResult::Single(t) => Ok(t.naive_utc()),
                LocalResult::Ambiguous(a, _) => Ok(a.naive_utc()),
                LocalResult::None => Err(anyhow!("Unresolvable local time")),
            }
        }
    }
}

fn parse_offset(z: &str) -> Result<i32> {
    let sign = if z.starts_with('-') { -1 } else { 1 };
    let body = z.trim_start_matches(['+', '-']).replace(':', "");
    let (h, m) = match body.len() {
        1 | 2 => (body.parse::<i32>()?, 0),
        3 => (body[..1].parse::<i32>()?, body[1..].parse::<i32>()?),
        _ => (body[..2].parse::<i32>()?, body[2..4].parse::<i32>()?),
    };
    Ok(sign * (h * 3600 + m * 60))
}



/// What `strftime` needs to know about a `Time`.
impl strftime::Time for RTime {
    fn year(&self) -> i32 { self.at.year() }
    fn month(&self) -> u8 { self.at.month() as u8 }
    fn day(&self) -> u8 { self.at.day() as u8 }
    fn hour(&self) -> u8 { self.at.hour() as u8 }
    fn minute(&self) -> u8 { self.at.minute() as u8 }
    fn second(&self) -> u8 { self.at.second() as u8 }
    fn nanoseconds(&self) -> u32 { self.at.nanosecond() }
    fn day_of_week(&self) -> u8 { self.at.weekday().num_days_from_sunday() as u8 }
    fn day_of_year(&self) -> u16 { self.at.ordinal() as u16 }
    fn to_int(&self) -> i64 { self.at.timestamp() }
    fn is_utc(&self) -> bool { self.zone.as_deref() == Some("UTC") }
    fn utc_offset(&self) -> i32 { self.at.offset().local_minus_utc() }
    // A `Time` made from a numeric offset has no zone name; `%Z` prints "".
    fn time_zone(&self) -> &str { self.zone.as_deref().unwrap_or("") }
}
