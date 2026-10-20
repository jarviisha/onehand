use super::*;

/// GitHub's #6 and #12, both open, and a closed draft, read the way the mode
/// reads them: from the file.
fn kept() -> Issues {
    let link = |n: u32| {
        format!(
            r##"{{"connector": "GitHub", "key": "{n}", "reference": "#{n}",
                "base": {{"title": "", "open": true}}}}"##
        )
    };
    let text = format!(
        r#"{{"next": 4, "issues": [
            {{"number": 1, "title": "Fix the footer", "labels": ["bug"], "link": {}}},
            {{"number": 2, "title": "Add search", "link": {}}},
            {{"number": 3, "title": "Old draft", "open": false}}
        ]}}"#,
        link(6),
        link(12)
    );
    // One file per call: the tests run side by side, and one removing the
    // file another is reading fails that one.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!(
        "onehand-issues-list-{}-{call}.json",
        std::process::id()
    ));
    std::fs::write(&file, text).unwrap();
    let issues = issues::load_blocking(&file).unwrap();
    let _ = std::fs::remove_file(&file);
    issues
}

fn titles(rows: &[&LocalIssue]) -> Vec<String> {
    rows.iter().map(|issue| issue.title.clone()).collect()
}

#[test]
fn a_hash_query_narrows_by_reference_and_names_the_exact_one() {
    let issues = kept();
    let by = |query| Narrowing { query, label: None };
    let (rows, open, closed) = listed(&issues, &by("#1"), Showing::Open);
    assert_eq!(titles(&rows), ["Add search"]);
    assert_eq!((open, closed), (1, 0));
    // `#1` is the start of `#12`, not an issue of its own: onehand's own
    // number for the first issue is never matched.
    assert!(named(&issues, "#1").is_none());
    assert_eq!(named(&issues, " #6 ").unwrap().title, "Fix the footer");
}

#[test]
fn a_word_narrows_by_title_and_the_counts_follow_it() {
    let issues = kept();
    let (rows, open, closed) = listed(
        &issues,
        &Narrowing {
            query: "DRAFT",
            label: None,
        },
        Showing::Open,
    );
    assert!(rows.is_empty());
    assert_eq!((open, closed), (0, 1));
}

#[test]
fn a_label_narrows_to_the_issues_carrying_it() {
    let issues = kept();
    let (rows, ..) = listed(
        &issues,
        &Narrowing {
            query: "",
            label: Some("bug"),
        },
        Showing::Open,
    );
    assert_eq!(titles(&rows), ["Fix the footer"]);
}
