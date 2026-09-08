//! Dumps rekyll's YAML scalar resolution in the same format as
//! `tests/harness/psych_ref.rb`, so the two can be diffed directly.

use rekyll::value::Value;

fn show(v: &Value) -> String {
    match v {
        Value::Null => "Null".into(),
        Value::Bool(b) => format!("Bool({b})"),
        Value::Int(i) => format!("Int({i})"),
        Value::Float(x) if x.is_nan() => "Float(NaN)".into(),
        Value::Float(x) => format!("Float({})", rekyll::value::ruby_float_to_s(*x)),
        Value::Str(s) => format!("Str({s})"),
        Value::Date { at, date_only } => {
            if *date_only {
                format!("Date({},date)", at.format("%Y-%m-%d"))
            } else {
                format!("Date({},time)", at.format("%Y-%m-%d %H:%M:%S %z"))
            }
        }
        Value::Array(_) => "Array".into(),
        Value::Object(_) => "Object".into(),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: scalars <file>");
    let text = std::fs::read_to_string(path).expect("read scalars file");
    for line in text.lines() {
        let doc = rekyll::yaml::load(&format!("v: {line}"));
        let out = match doc {
            Ok(v) => v.get("v").map(show).unwrap_or_else(|| "MISSING".into()),
            Err(e) => format!("ERROR({e})"),
        };
        println!("{line}\t{out}");
    }
}
