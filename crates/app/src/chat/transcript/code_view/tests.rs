use super::*;

#[test]
fn the_cap_holds_sixteen_lines() {
    assert_eq!(lines_shown(), 16);
}

#[test]
fn a_block_that_fits_holds_nothing_back() {
    assert_eq!(held_back(""), None);
    assert_eq!(held_back(&"x\n".repeat(16)), None);
    assert_eq!(held_back(&"x\n".repeat(17)), Some(17));
}

#[test]
fn the_dialog_cuts_at_its_cap_and_says_how_many() {
    let code = "a\nb\nc\n";
    assert_eq!(capped(code, 2), ("a\nb", Some(3)));
    assert_eq!(capped(code, 3), (code, None));
}

#[test]
fn the_fence_outruns_any_run_inside() {
    assert_eq!(
        fence("let x = 1;", Some("rust")),
        "```rust\nlet x = 1;\n```"
    );
    let inner = "````md\n```\n````";
    assert!(fence(inner, None).starts_with("`````\n"));
    assert!(fence(inner, None).ends_with("\n`````"));
}
