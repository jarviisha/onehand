use super::*;

#[test]
fn priority_is_the_first_line_of_its_section() {
    let body = "Intro\n\n## Priority\n\n- **High**: the footer lies\n- second\n\n## Steps\n1. x";
    assert_eq!(priority(body).as_deref(), Some("High: the footer lies"));
    assert_eq!(priority("### priority:\nP1\n").as_deref(), Some("P1"));
    // A section with nothing under it before the next heading says nothing.
    assert_eq!(priority("## Priority\n\n## Steps\nP1"), None);
    assert_eq!(priority("Priority is high"), None);
    // Bold with no list marker: the opening `*` is the bold's, not a marker.
    assert_eq!(priority("## Priority\n**High**").as_deref(), Some("High"));
}

#[test]
fn a_priority_presses_by_its_words() {
    assert_eq!(urgency("Critical"), 2);
    assert_eq!(urgency("P0 — now"), 2);
    assert_eq!(urgency("High: the footer lies"), 1);
    assert_eq!(urgency("p1"), 1);
    assert_eq!(urgency("Low"), -1);
    assert_eq!(urgency("Lowest of the medium items"), -1);
    assert_eq!(urgency("Medium"), 0);
    // A word inside another is not that word.
    assert_eq!(urgency("Highlight later"), 0);
    assert_eq!(priority_name("High: the footer lies"), "High");
}
