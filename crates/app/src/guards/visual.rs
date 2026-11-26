//! Guards on where a colour and a size may be written.
//!
//! clippy holds the calls (`clippy.toml`); these hold what is not a call: a
//! colour spelled as a string or a struct, a size spelled as a number. They
//! read production code only. A test measures against whatever numbers it
//! likes, and holding it to these rules would bury the real offenders in
//! fixtures.

use super::{ui_sources, workspace_root};

/// The files a parent declares test code: for every `mod NAME;` under
/// `#[cfg(test)]`, the module's file and the directory of its children, as
/// path prefixes (`<dir>/NAME.rs`, `<dir>/NAME/`).
///
/// A test rig is a module of its own, not named `tests.rs`, and only its
/// parent's attribute says it never ships. A module declared through a
/// `#[path]`, or under a cfg that is not plain `test`, is not seen.
fn test_only_files(sources: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (path, source) in sources {
        let dir = match path.rsplit('/').next() {
            Some("lib.rs" | "main.rs" | "mod.rs") => path
                .rsplit_once('/')
                .map_or(String::new(), |(dir, _)| dir.to_string()),
            _ => path.trim_end_matches(".rs").to_string(),
        };
        let lines: Vec<&str> = source.lines().map(str::trim).collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(rest) = line.strip_prefix("#[cfg(test)]") else {
                continue;
            };
            // The item may share the attribute's line, or follow it under
            // more attributes.
            let item = match rest.trim() {
                "" => lines[i + 1..]
                    .iter()
                    .find(|l| !l.starts_with("#["))
                    .copied()
                    .unwrap_or_default(),
                rest => rest,
            };
            if let Some(name) = declared_module(item) {
                out.push(format!("{dir}/{name}.rs"));
                out.push(format!("{dir}/{name}/"));
            }
        }
    }
    out
}

/// The name in a `mod NAME;` line, whatever its visibility.
fn declared_module(item: &str) -> Option<&str> {
    let item = match item.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')')?.1,
        Some(rest) => rest,
        None => item,
    };
    let name = item.trim_start().strip_prefix("mod ")?.strip_suffix(';')?;
    Some(name.trim())
}

/// The production lines of one file, each with its 1-based number, or `None`
/// for a file that is test code as a whole: a `tests.rs`, anything under a
/// `tests/` directory, or a module its parent declares under `#[cfg(test)]`
/// (`test_only` is [`test_only_files`]).
///
/// An item under `#[cfg(test)]` (a module, a function, a `mod NAME;` line) is
/// left out with its attributes, its end found by counting braces from its
/// first line. A brace in a string or a char that is not matched inside the
/// same item throws the count off.
fn without_tests(path: &str, source: &str, test_only: &[String]) -> Option<Vec<(usize, String)>> {
    if path.ends_with("/tests.rs")
        || path.contains("/tests/")
        || test_only
            .iter()
            .any(|prefix| path == prefix || (prefix.ends_with('/') && path.starts_with(prefix)))
    {
        return None;
    }
    let mut out = Vec::new();
    // Inside a test item: whether its body has opened, and how deep it is.
    let mut skipping: Option<(bool, usize)> = None;
    for (i, line) in source.lines().enumerate() {
        let t = line.trim();
        let mut item = t;
        if skipping.is_none() {
            match t.strip_prefix("#[cfg(test)]") {
                Some(rest) => {
                    skipping = Some((false, 0));
                    item = rest.trim();
                }
                None => {
                    out.push((i + 1, line.to_string()));
                    continue;
                }
            }
        }
        let Some((opened, depth)) = skipping.as_mut() else {
            continue;
        };
        if item.is_empty() || (!*opened && item.starts_with("#[")) {
            continue;
        }
        let opens = item.matches('{').count();
        let closes = item.matches('}').count();
        *opened |= opens > 0;
        *depth = (*depth + opens).saturating_sub(closes);
        if (*opened && *depth == 0) || (!*opened && item.ends_with(';')) {
            skipping = None;
        }
    }
    Some(out)
}

/// Every UI source's production lines, as (path, lines).
fn production() -> Vec<(String, Vec<(usize, String)>)> {
    let sources = ui_sources();
    let test_only = test_only_files(&sources);
    sources
        .iter()
        .filter_map(|(path, source)| {
            without_tests(path, source, &test_only).map(|lines| (path.clone(), lines))
        })
        .collect()
}

/// Whether `line` holds a hex colour string: `"#` then six or eight hex
/// digits, then `"`. Three and four digits are left out, because an issue
/// reference (`"#100"`) looks exactly like one.
fn holds_hex_colour(line: &str) -> bool {
    line.match_indices("\"#").any(|(at, _)| {
        let digits: String = line[at + 2..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        matches!(digits.len(), 6 | 8) && line[at + 2 + digits.len()..].starts_with('"')
    })
}

/// Whether `line` builds an `Rgba` or an `Hsla` as a struct literal, rather
/// than naming the type in a signature or a declaration.
fn holds_colour_literal(line: &str) -> bool {
    ["Rgba {", "Hsla {"].iter().any(|name| {
        line.match_indices(name).any(|(at, _)| {
            let before = line[..at].trim_end_matches("gpui::");
            if before.ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                return false;
            }
            let before = before.trim_end();
            !(before.ends_with("->")
                || before.ends_with(" for")
                || before.ends_with("impl")
                || before.ends_with("struct"))
        })
    })
}

/// Whether `line` holds a `px(` or `rems(` whose whole argument is a number
/// other than zero.
fn holds_size_number(line: &str) -> bool {
    ["px(", "rems("].iter().any(|call| {
        line.match_indices(call).any(|(at, _)| {
            if line[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                return false;
            }
            let rest = &line[at + call.len()..];
            let Some((arg, _)) = rest.split_once(')') else {
                return false;
            };
            arg.trim()
                .replace('_', "")
                .parse::<f32>()
                .is_ok_and(|n| n != 0.)
        })
    })
}

/// The name of the `const` item a line opens, and what follows its colon,
/// if it opens one; `const fn` is a function, not a constant.
fn opens_const(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let t = match t.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')')?.1.trim_start(),
        Some(rest) => rest.trim_start(),
        None => t,
    };
    let (name, rest) = t.strip_prefix("const ")?.split_once(':')?;
    let name = name.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then_some((name, rest))
}

#[test]
fn test_code_is_left_out_of_a_scan() {
    let sample = "fn kept() {}\n\
                  #[cfg(test)]\n\
                  mod tests {\n\
                  \x20   fn t() {\n\
                  \x20       let s = format!(\"{x}\");\n\
                  \x20   }\n\
                  }\n\
                  #[cfg(test)] fn helper() {}\n\
                  fn also_kept() {}";
    let kept = without_tests("x/src/a.rs", sample, &[]).expect("a source file");
    assert_eq!(
        kept,
        vec![
            (1, "fn kept() {}".to_string()),
            (9, "fn also_kept() {}".to_string())
        ]
    );
    assert_eq!(without_tests("x/tests.rs", sample, &[]), None);
    assert_eq!(without_tests("x/tests/a.rs", sample, &[]), None);

    let parents = [
        (
            "x/src/shell.rs".to_string(),
            "#[cfg(test)]\npub(crate) mod rig;\nmod roots;".to_string(),
        ),
        (
            "x/src/lib.rs".to_string(),
            "#[cfg(test)]\nmod guards;".to_string(),
        ),
    ];
    let test_only = test_only_files(&parents);
    assert_eq!(
        test_only,
        [
            "x/src/shell/rig.rs",
            "x/src/shell/rig/",
            "x/src/guards.rs",
            "x/src/guards/"
        ]
    );
    assert_eq!(without_tests("x/src/shell/rig.rs", "", &test_only), None);
    assert_eq!(
        without_tests("x/src/shell/rig/strain.rs", "", &test_only),
        None
    );
    assert!(without_tests("x/src/shell/roots.rs", "", &test_only).is_some());
}

/// A colour is written in the palette alone.
///
/// The lint holds the calls that make a colour; this holds the ones spelled
/// out: a hex string, or an `Rgba`/`Hsla` built field by field. The palette
/// and the terminal's sixteen colours are where they live. A three-digit hex,
/// or a colour built from numbers some other way, is not seen.
#[test]
fn a_colour_is_written_in_the_palette_alone() {
    assert!(holds_hex_colour(r##"list_even: "#18181800","##));
    assert!(!holds_hex_colour(r##"Ok("#100".to_string())"##));
    assert!(!holds_colour_literal(
        "pub fn dock_surface(cx: &App) -> Hsla {"
    ));
    assert!(!holds_colour_literal("fn ink(cx: &App) -> gpui::Hsla {"));
    assert!(holds_colour_literal(
        "let ink = Hsla { h: 0., s: 0., l: 0., a: 1. };"
    ));

    let mut palette = 0;
    let mut offenders = Vec::new();
    for (path, lines) in production() {
        let home = path.ends_with("crates/app/src/theme.rs")
            || path.ends_with("crates/terminal-ui/src/ansi.rs");
        for (n, line) in lines {
            if line.trim_start().starts_with("//") {
                continue;
            }
            match home {
                true => palette += usize::from(holds_hex_colour(&line)),
                false if holds_hex_colour(&line) || holds_colour_literal(&line) => {
                    offenders.push(format!("{path}:{n}"));
                }
                false => {}
            }
        }
    }
    assert!(
        palette > 50,
        "found {palette} colours in the palette -- this scan has stopped scanning"
    );
    assert!(
        offenders.is_empty(),
        "a colour written outside the palette at {offenders:?}: give it a slot in the \
         palette and read it from the theme."
    );
}

/// A size is written once, in a constant.
///
/// A `px(` or `rems(` holding a number other than zero stands only in a
/// `const` item, so every size has a name and the census below can list it.
/// The spacing and type steps (`.gap_2()`, `.text_xs()`) are the scale itself
/// and are not read, nor is a size computed from other constants or held in a
/// bare `f32`.
#[test]
fn a_size_is_written_once_in_a_constant() {
    let mut constants = 0;
    let mut offenders = Vec::new();
    for (path, lines) in production() {
        let mut in_const = false;
        for (n, line) in lines {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if !in_const && opens_const(t).is_some() {
                in_const = true;
                constants += usize::from(t.contains("Rems") || t.contains("Pixels"));
            }
            if in_const {
                in_const = !t.contains(';');
                continue;
            }
            if holds_size_number(&line) {
                offenders.push(format!("{path}:{n}"));
            }
        }
    }
    assert!(
        constants > 150,
        "found {constants} size constants -- this scan has stopped scanning"
    );
    assert!(
        offenders.is_empty(),
        "a size written as a number outside a constant at {offenders:?}: give it a name \
         in a `const` beside its use."
    );
}

/// The census of one file's sizes, as `<path>  <NAME> = <value as written>`.
///
/// A constant typed `Rems` or `Pixels`, every constant of the shared tokens
/// whatever its type, and every slot of the palette, its dark value then its
/// light one. A value is the source's own text, its whitespace runs joined
/// to one space, so a constant computed from others or written over two
/// lines reads as the source has it.
fn census_of(path: &str, lines: &[(usize, String)]) -> Vec<String> {
    let tokens = path == "crates/ui/src/tokens.rs";
    let palette = path == "crates/app/src/theme.rs";
    let mut out = Vec::new();
    let mut item: Option<String> = None;
    for (_, line) in lines {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        if palette
            && let Some((slot, values)) = t.split_once(": \"#")
            && slot.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        {
            let values = values.trim_end_matches(';');
            out.push(format!("{path}  {slot} = \"#{values}"));
            continue;
        }
        match item.as_mut() {
            Some(text) => {
                text.push(' ');
                text.push_str(t);
            }
            None if opens_const(t).is_some() => item = Some(t.to_string()),
            None => continue,
        }
        let Some(text) = item.take_if(|text| text.contains(';')) else {
            continue;
        };
        let shared = tokens && text.starts_with("pub const ");
        let Some((name, rest)) = opens_const(&text) else {
            continue;
        };
        let Some((ty, value)) = rest.split_once('=') else {
            continue;
        };
        let ty = ty.trim().trim_start_matches("gpui::");
        if shared || ty == "Rems" || ty == "Pixels" {
            let value = value.split_once(';').map_or(value, |(value, _)| value);
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(format!("{path}  {name} = {value}"));
        }
    }
    out
}

/// Every size and palette value of the UI, sorted, one line each.
fn census() -> Vec<String> {
    let root = format!("{}/", workspace_root().display());
    let mut out: Vec<String> = production()
        .iter()
        .flat_map(|(path, lines)| census_of(path.strip_prefix(&root).unwrap_or(path), lines))
        .collect();
    out.sort();
    out
}

#[test]
fn the_census_reads_a_constant_as_written() {
    let lines: Vec<(usize, String)> = [
        "const A: Rems = rems(1.);",
        "const B: Pixels =",
        "    px(2.);",
        "pub(crate) const C: gpui::Rems = rems(D.0 * 2.);",
        "pub(in crate::chat) const E: Rems = rems(3.);",
        "const N: usize = 3;",
        "const fn half() -> Rems { rems(0.5) }",
    ]
    .iter()
    .enumerate()
    .map(|(i, line)| (i + 1, line.to_string()))
    .collect();
    assert_eq!(
        census_of("x/a.rs", &lines),
        [
            "x/a.rs  A = rems(1.)",
            "x/a.rs  B = px(2.)",
            "x/a.rs  C = rems(D.0 * 2.)",
            "x/a.rs  E = rems(3.)"
        ]
    );
}

/// The shared sizes, the local ones and the palette are the listed ones.
///
/// A size added, changed or removed fails here until `sizes.txt` says so, so
/// a change to the foundation shows by name in the diff that makes it. The
/// list is brought up to date by hand from this test's own words; nothing is
/// written on the machine that runs it.
#[test]
fn the_sizes_are_the_listed_ones() {
    let found = census();
    let listed: Vec<&str> = include_str!("sizes.txt")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(
        found.len() > 200,
        "found {} sizes -- this census has stopped counting",
        found.len()
    );
    let add: Vec<String> = found
        .iter()
        .filter(|line| !listed.contains(&line.as_str()))
        .map(|line| format!("+ {line}"))
        .collect();
    let drop: Vec<String> = listed
        .iter()
        .filter(|line| !found.iter().any(|f| f == *line))
        .map(|line| format!("- {line}"))
        .collect();
    assert!(
        add.is_empty() && drop.is_empty(),
        "the sizes differ from sizes.txt.\n\
         A shared size or a palette value changes only when the owner asked for it; \
         bring the list up to date by hand from these lines.\n{}\n{}",
        add.join("\n"),
        drop.join("\n")
    );
}
