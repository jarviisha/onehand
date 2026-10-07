use super::*;

fn bug() -> IssueTemplate {
    shipped().into_iter().find(|t| t.name == "Bug").unwrap()
}

#[test]
fn three_templates_ship_each_with_the_four_headings() {
    let all = shipped();
    let names: Vec<_> = all.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Bug", "Feature", "Refactor"]);
    for template in &all {
        let headings: Vec<_> = headings(&template.body)
            .into_iter()
            .map(|h| h.text)
            .collect();
        assert_eq!(
            headings,
            ["problem", "scope", "acceptance", "how to check"],
            "{}",
            template.name
        );
    }
    assert_eq!(bug().labels, ["bug"]);
}

#[test]
fn a_template_untouched_lacks_every_section() {
    let lacks = lacking(&bug().body, &shipped()).unwrap();
    assert_eq!(
        lacks.missing,
        ["Problem", "Scope", "Acceptance", "How to check"]
    );
}

#[test]
fn only_acceptance_left_as_its_hint_is_said() {
    let body = "## Problem\nIt crashes.\n\n## Scope\nThe parser.\n\n## Acceptance\n<!-- How you will judge the work. -->\n\n## How to check\n`cargo test`\n";
    let lacks = lacking(body, &shipped()).unwrap();
    assert_eq!(lacks.missing, ["Acceptance"]);
    assert_eq!(lacks.said(), "No acceptance written");
}

#[test]
fn heading_levels_case_and_space_do_not_matter() {
    let body = "### problem  \nIt crashes.\n#   SCOPE\nThe parser.\n";
    let lacks = lacking(body, &shipped()).unwrap();
    assert_eq!(lacks.missing, ["Acceptance", "How to check"]);
    assert_eq!(lacks.said(), "No acceptance or how to check written");
}

#[test]
fn a_comment_spanning_lines_is_not_content() {
    let body = "## Problem\n<!-- one\ntwo -->\n## Scope\nx\n## Acceptance\ny\n## How to check\nz\n";
    assert_eq!(lacking(body, &shipped()).unwrap().missing, ["Problem"]);
}

#[test]
fn a_section_holding_only_a_sub_heading_is_empty_but_one_with_text_under_it_is_not() {
    let body = "## Problem\n### Details\n## Scope\n### Parts\nThe parser.\n## Acceptance\ny\n## How to check\nz\n";
    assert_eq!(lacking(body, &shipped()).unwrap().missing, ["Problem"]);
}

#[test]
fn a_heading_inside_a_fence_is_not_one() {
    let body = "## Problem\n```sh\n# Scope\n```\n## Acceptance\ny\n";
    let lacks = lacking(body, &shipped()).unwrap();
    // The fence is the problem's content; its `# Scope` is no heading.
    assert_eq!(lacks.missing, ["Scope", "How to check"]);
}

#[test]
fn a_body_matches_with_half_the_headings_and_not_with_fewer() {
    let half = "## Problem\nx\n## Scope\ny\n";
    assert!(lacking(half, &shipped()).is_some());
    let fewer = "## Problem\nx\n## Notes\ny\n";
    assert_eq!(lacking(fewer, &shipped()), None);
}

#[test]
fn a_body_written_its_own_way_gets_no_advice() {
    assert_eq!(lacking("It crashes when I open a file.", &shipped()), None);
    assert_eq!(
        lacking("## Steps\n1. open\n## Expected\nno crash", &shipped()),
        None
    );
    assert_eq!(lacking("", &shipped()), None);
}

#[test]
fn a_body_whose_sections_are_all_written_gets_no_advice() {
    let body = "## Problem\na\n## Scope\nb\n## Acceptance\nc\n## How to check\nd\n";
    assert_eq!(lacking(body, &shipped()), None);
}

#[test]
fn the_template_with_more_headings_in_the_body_is_the_one_read() {
    let own = IssueTemplate {
        name: "Own".into(),
        body: "## Steps\n## Expected\n## Seen\n".into(),
        labels: Vec::new(),
    };
    let body = "## Steps\n1. open\n## Expected\n## Seen\ncrash\n## Problem\nx\n";
    let lacks = lacking(body, &[shipped().remove(0), own]).unwrap();
    assert_eq!(lacks.missing, ["Expected"]);
}

/// A section is read as written, its hints left out; an empty or absent one
/// is none.
#[test]
fn a_section_is_read_without_its_hints() {
    let body = "## Problem\nIt breaks.\n\n## Acceptance\n<!-- How you will judge it. -->\n\
                - It no longer breaks.\n- A test says so.\n\n## How to check\n`make test`\n";
    assert_eq!(
        section(body, "Acceptance").as_deref(),
        Some("- It no longer breaks.\n- A test says so.")
    );
    assert_eq!(section(&bug().body, "Acceptance"), None, "only the hint");
    assert_eq!(section("Free text", "Acceptance"), None);
}

#[test]
fn a_project_template_reads_its_name_and_labels_from_front_matter() {
    let text = "---\nname: Bug report\nabout: Something is broken\ntitle: ''\nlabels: bug, \"needs triage\"\nassignees: ''\n---\n\n## What happened\n\n## Expected\n";
    let template = from_file(text).unwrap();
    assert_eq!(template.name, "Bug report");
    assert_eq!(template.labels, ["bug", "needs triage"]);
    assert_eq!(template.body, "## What happened\n\n## Expected\n");
}

#[test]
fn labels_read_as_a_flow_or_a_block_list() {
    let flow = from_file("---\nname: A\nlabels: [\"bug\", 'ui']\n---\nbody\n").unwrap();
    assert_eq!(flow.labels, ["bug", "ui"]);
    let block = from_file("---\nname: B\nlabels:\n  - bug\n  - 'ui'\nabout: x\n---\nbody\n").unwrap();
    assert_eq!(block.labels, ["bug", "ui"]);
    let none = from_file("---\nname: C\nlabels: ''\n---\nbody\n").unwrap();
    assert!(none.labels.is_empty());
}

#[test]
fn a_file_without_front_matter_or_a_name_is_left_out() {
    assert_eq!(from_file("## Problem\n\n## Scope\n"), None);
    assert_eq!(from_file("---\nabout: no name\n---\nbody\n"), None);
    assert_eq!(from_file("---\nname: never closed\nbody\n"), None);
}

#[test]
fn a_project_template_replaces_the_shipped_ones_for_the_body_reader() {
    let own = from_file("---\nname: Story\n---\n## Why\n\n## Done when\n").unwrap();
    let body = "## Why\nUsers ask for it.\n\n## Done when\n";
    let lacks = lacking(body, std::slice::from_ref(&own)).unwrap();
    assert_eq!(lacks.missing, ["Done when"]);
    // Written from the shipped bug template, it matches nothing of the
    // project's, so it is never told it lacks anything.
    assert_eq!(lacking(&bug().body, &[own]), None);
}

#[test]
fn a_project_with_no_readable_template_keeps_the_shipped_three() {
    assert_eq!(own_or_shipped(Vec::new()), shipped());
    let own = from_file("---\nname: Story\n---\n## Why\n").unwrap();
    assert_eq!(own_or_shipped(vec![own.clone()]), vec![own]);
}
