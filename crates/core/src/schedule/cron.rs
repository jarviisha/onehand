//! The five-field cron expression a schedule keeps: minute, hour, day of
//! month, month, day of week, each `*`, a number, a range `a-b`, a list
//! `a,b` or a step `*/n` or `a-b/n`. Written here rather than taken from a
//! crate: this is all a schedule needs, and it is a page.

use chrono::{Datelike, Timelike};

/// A parsed expression: one bit per value each field allows.
#[derive(Clone, Debug, PartialEq)]
pub struct Cron {
    minute: u64,
    hour: u64,
    day: u64,
    month: u64,
    weekday: u64,
    /// Whether day of month and day of week were written `*`. Cron's own
    /// rule: when both are restricted, a day matching either one is a match.
    any_day: bool,
    any_weekday: bool,
}

/// Each field's name, as an error says it, and its range.
const FIELDS: [(&str, u32, u32); 5] = [
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day of month", 1, 31),
    ("month", 1, 12),
    ("day of week", 0, 7),
];

/// Read `expr`, or say which field is wrong and how.
pub fn parse(expr: &str) -> Result<Cron, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!("5 fields expected, found {}", parts.len()));
    }
    let mut bits = [0u64; 5];
    for (i, part) in parts.iter().enumerate() {
        let (name, min, max) = FIELDS[i];
        bits[i] = field(part, min, max).map_err(|why| format!("{name}: {why}"))?;
    }
    // Sunday is 0 and 7 alike.
    if bits[4] & (1 << 7) != 0 {
        bits[4] = (bits[4] & !(1 << 7)) | 1;
    }
    Ok(Cron {
        minute: bits[0],
        hour: bits[1],
        day: bits[2],
        month: bits[3],
        weekday: bits[4],
        // A field written from `*`, a step on it included, counts as `*` for
        // that rule, as cron reads it.
        any_day: parts[2].starts_with('*'),
        any_weekday: parts[4].starts_with('*'),
    })
}

/// The values one field allows, as bits.
fn field(text: &str, min: u32, max: u32) -> Result<u64, String> {
    let mut bits = 0u64;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step.parse().map_err(|_| format!("{step} is not a step"))?;
                if step == 0 {
                    return Err("a step of 0".to_string());
                }
                (range, step)
            }
            None => (item, 1),
        };
        let (from, to) = match range {
            "*" => (min, max),
            _ => match range.split_once('-') {
                Some((a, b)) => (number(a, min, max)?, number(b, min, max)?),
                // A step from one number runs to the field's end, as cron
                // reads `5/15`: 5, 20, 35, 50.
                None => {
                    let n = number(range, min, max)?;
                    (n, if item.contains('/') { max } else { n })
                }
            },
        };
        if from > to {
            return Err(format!("{from}-{to} runs backwards"));
        }
        for n in (from..=to).step_by(step as usize) {
            bits |= 1 << n;
        }
    }
    Ok(bits)
}

fn number(text: &str, min: u32, max: u32) -> Result<u32, String> {
    let n: u32 = text
        .parse()
        .map_err(|_| format!("{text} is not a number"))?;
    match (n < min, n > max) {
        (true, _) => Err(format!("{n} is before {min}")),
        (_, true) => Err(format!("{n} is past {max}")),
        _ => Ok(n),
    }
}

impl Cron {
    /// Whether the local minute `t` is one the expression names.
    pub(crate) fn matches(&self, t: &(impl Datelike + Timelike)) -> bool {
        self.day_matches(t) && self.hour_matches(t) && has(self.minute, t.minute())
    }

    /// Whether the expression names some minute of `t`'s day: what lets a
    /// search step over a whole day at once.
    pub(crate) fn day_matches(&self, t: &impl Datelike) -> bool {
        let day = has(self.day, t.day());
        let weekday = has(self.weekday, t.weekday().num_days_from_sunday());
        let on_day = match (self.any_day, self.any_weekday) {
            (false, false) => day || weekday,
            _ => day && weekday,
        };
        has(self.month, t.month()) && on_day
    }

    /// Whether the expression names some minute of `t`'s hour.
    pub(crate) fn hour_matches(&self, t: &impl Timelike) -> bool {
        has(self.hour, t.hour())
    }
}

fn has(bits: u64, n: u32) -> bool {
    bits & (1 << n) != 0
}
