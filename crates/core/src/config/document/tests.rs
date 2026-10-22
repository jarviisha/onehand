use super::*;

/// `merge(text, before, after)` as text: `before` and `after` given as TOML.
fn merged(text: &str, before: &str, after: &str) -> String {
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
    let before: toml::Table = toml::from_str(before).unwrap();
    let after: toml::Table = toml::from_str(after).unwrap();
    merge(&mut doc, &before, &after);
    doc.to_string()
}

#[test]
fn a_save_that_changes_nothing_leaves_the_file_alone() {
    let text = "# mine\nappearance = \"dark\"  # chosen\n\n[font]\nmonospace = \"X\"\n";
    assert_eq!(merged(text, text, text), text);
}

#[test]
fn a_changed_value_keeps_comments_and_unknown_keys() {
    let text = "# mine\nappearance = \"dark\"\nfuture = 1  # newer build\n\n\
                [remote.telegram]\nenabled = false\nfuture_nested = \"keep\"\n";
    let out = merged(
        text,
        "appearance = \"dark\"\n[remote.telegram]\nenabled = false\n",
        "appearance = \"light\"\n[remote.telegram]\nenabled = true\n",
    );
    assert!(out.contains("# mine"), "{out}");
    assert!(out.contains("future = 1  # newer build"), "{out}");
    assert!(out.contains("future_nested = \"keep\""), "{out}");
    assert!(out.contains("appearance = \"light\""), "{out}");
    assert!(out.contains("enabled = true"), "{out}");
    assert!(toml::from_str::<toml::Table>(&out).is_ok(), "{out}");
}

#[test]
fn a_value_gone_from_the_config_leaves_the_file() {
    let text = "[keymap]\nrestart = [\"alt-r\"]\ntoggle = [\"alt-j\"]\n";
    let out = merged(text, text, "[keymap]\ntoggle = [\"alt-j\"]\n");
    assert!(!out.contains("restart"), "{out}");
    assert!(out.contains("toggle = [\"alt-j\"]"), "{out}");
}

#[test]
fn a_new_bare_key_lands_above_the_tables() {
    let text = "[[agents]]\nname = \"A\"\ncommand = \"a\"\n";
    let out = merged(text, text, &format!("appearance = \"light\"\n{text}"));
    let back: toml::Table = toml::from_str(&out).unwrap();
    assert_eq!(back["appearance"].as_str(), Some("light"), "{out}");
    let agents = back["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["name"].as_str(), Some("A"));
}

#[test]
fn dotted_and_inline_tables_are_merged_in_place() {
    let text = "remote.telegram.enabled = true\nfont = { monospace = \"X\" }\n";
    let out = merged(
        text,
        text,
        "remote.telegram.enabled = false\nfont = { monospace = \"Y\" }\n",
    );
    assert!(out.contains("remote.telegram.enabled = false"), "{out}");
    assert!(out.contains("font = { monospace = \"Y\" }"), "{out}");
    assert!(!out.contains("[remote") && !out.contains("[font]"), "{out}");
}
