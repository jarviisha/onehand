use super::*;

fn paths(body: &str) -> Vec<String> {
    mentions(body).into_iter().map(|m| m.path).collect()
}

#[test]
fn paths_are_found_beside_punctuation_and_in_code() {
    let body = "See crates/a.rs, then (crates/b.rs). Also `c/d.rs`: done.\n\
                Ends a sentence: src/e.rs. Quoted \"src/f.rs\"; line ./src/g.rs:42:7!";
    assert_eq!(
        paths(body),
        [
            "crates/a.rs",
            "crates/b.rs",
            "c/d.rs",
            "src/e.rs",
            "src/f.rs",
            "src/g.rs"
        ]
    );
}

#[test]
fn what_only_looks_like_a_path_is_left_alone() {
    let body = "open/closed is a word pair, /etc/hosts is absolute, ../up climbs, \
                https://x.com/a/b.rs is an address, [x](docs/a.md) is a link, [`src/a.rs`](x) too\n\
                ```\nfenced/e.rs\n```\n    indented/f.rs\n`no slash.rs`";
    // `open/closed` is shaped like a path; only the existence check, which
    // these tests do not run, keeps it plain.
    assert_eq!(paths(body), ["open/closed"]);
}

#[test]
fn a_mention_covers_the_path_and_not_the_punctuation() {
    let body = "Fix (src/a.rs).";
    let found = mentions(body);
    assert_eq!(&body[found[0].range.clone()], "src/a.rs");
    let body = "In `src/a.rs:3` here";
    assert_eq!(&body[mentions(body)[0].range.clone()], "`src/a.rs:3`");
}

#[test]
fn only_paths_that_exist_become_links() {
    let body = "Edit `src/a.rs:3` and src/b.rs, not src/gone.rs.";
    let found = mentions(body);
    let exists = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
    assert_eq!(
        linked(body, &found, &exists),
        "Edit [`src/a.rs:3`](<onehand-file:src/a.rs>) and \
         [`src/b.rs`](<onehand-file:src/b.rs>), not src/gone.rs."
    );
}
