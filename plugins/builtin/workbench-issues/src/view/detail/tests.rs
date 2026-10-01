use super::*;

#[test]
fn priority_is_the_first_line_of_its_section() {
    let body = "Intro\n\n## Priority\n\n- **High**: the footer lies\n- second\n\n## Steps\n1. x";
    assert_eq!(priority(body).as_deref(), Some("High: the footer lies"));
    assert_eq!(priority("### priority:\nP1\n").as_deref(), Some("P1"));
    // A section with nothing under it before the next heading says nothing.
    assert_eq!(priority("## Priority\n\n## Steps\nP1"), None);
    assert_eq!(priority("Priority is high"), None);
}
