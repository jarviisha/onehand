//! Source scans for the lab's rules the compiler cannot express: a size
//! spelled as arithmetic on a token instead of a token of its own (sixteen of
//! them had crept in), a colour outside the palette, and a button that skips
//! the wrapper giving it the pointer, the rule the app's own scan holds.

use std::path::Path;

/// Every source file but the ones allowed to hold what is scanned for, with
/// its test module cut off. Only `mod tests` is cut: `main.rs` declares this
/// module behind the same attribute, and the window's code follows it.
/// Submodules (`rail/tree.rs`) are scanned too.
fn sources(skip: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("a source dir") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            scan(&path, skip, &mut out);
        }
    }
    out
}

fn scan(path: &Path, skip: &[&str], out: &mut Vec<(String, String)>) {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    if !name.ends_with(".rs") || name == "guards.rs" || skip.contains(&name.as_str()) {
        return;
    }
    let text = std::fs::read_to_string(path).expect("a source file");
    let code = text
        .split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or("")
        .to_string();
    out.push((name, code));
}

/// The arguments of every `call(` in `code`, up to its matching parenthesis.
fn arguments<'a>(code: &'a str, call: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find(call) {
        let start = from + at + call.len();
        let before = code[..from + at].chars().last();
        from = start;
        // `rems(` but not `to_rems(`. A method call (`.opacity(`) always
        // follows its receiver's last letter, so it is never skipped.
        if !call.starts_with('.') && before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let mut depth = 1;
        for (i, c) in code[start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                out.push(&code[start..start + i]);
                break;
            }
        }
    }
    out
}

/// A number written into a length. A whole multiplier or divisor (`* 2.0`,
/// two sides of a box) is arithmetic on a token; anything else is a value
/// that belongs in `tokens.rs` with its reason beside it.
fn literal_in(arg: &str) -> Option<String> {
    let bytes = arg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        let starts = c.is_ascii_digit()
            && (i == 0 || !(bytes[i - 1] as char).is_alphanumeric() && bytes[i - 1] != b'_');
        if !starts {
            i += 1;
            continue;
        }
        let end = arg[i..]
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_'))
            .map_or(arg.len(), |n| i + n);
        let number = &arg[i..end];
        let operator = arg[..i].trim_end().chars().last();
        let whole = number
            .split_once('.')
            .is_none_or(|(_, frac)| frac.chars().all(|c| c == '0' || c == '_'));
        if !(whole && matches!(operator, Some('*' | '/'))) {
            return Some(number.to_string());
        }
        i = end;
    }
    None
}

#[test]
fn every_length_is_a_token() {
    let mut found = Vec::new();
    for (file, code) in sources(&["tokens.rs"]) {
        for call in ["rems(", "px("] {
            for arg in arguments(&code, call) {
                if let Some(n) = literal_in(arg) {
                    found.push(format!("{file}: {call}{arg}) has {n}"));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "name these in tokens.rs:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_colour_comes_from_the_palette() {
    let mut found = Vec::new();
    for (file, code) in sources(&["tokens.rs"]) {
        for call in ["rgb(", "rgba(", "hsla("] {
            if !arguments(&code, call).is_empty() {
                found.push(format!("{file}: {call}"));
            }
        }
        for arg in arguments(&code, ".opacity(") {
            if literal_in(arg).is_some() {
                found.push(format!("{file}: .opacity({arg})"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "use a Palette role:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_button_goes_through_the_wrapper() {
    let found: Vec<_> = sources(&["controls.rs"])
        .into_iter()
        .filter(|(_, code)| code.contains("Button::new("))
        .map(|(file, _)| file)
        .collect();
    assert!(found.is_empty(), "use controls::action in: {found:?}");
}

#[test]
fn the_scan_tells_a_token_from_a_number() {
    assert_eq!(literal_in("GUTTER * 0.5").as_deref(), Some("0.5"));
    assert_eq!(literal_in("16.0").as_deref(), Some("16.0"));
    assert_eq!(literal_in("GUTTER * 2.0"), None);
    assert_eq!(literal_in("TEXT_READ_SM * LEADING_READ"), None);
    assert_eq!(literal_in("ROW_H2"), None);
    assert_eq!(arguments("x.w(rems(a(b)))", "rems("), vec!["a(b)"]);
    assert!(arguments("to_rems(1.0)", "rems(").is_empty());
    assert_eq!(arguments("ink.opacity(0.0)", ".opacity("), vec!["0.0"]);
}
