use super::*;

#[test]
fn path_is_read_after_the_last_marker() {
    let out =
        format!("Welcome to the box!\n{MARKER} was here\n\n{MARKER}/usr/bin:/home/u/.nvm/bin");
    assert_eq!(
        after_marker(out.as_bytes()),
        Some(&b"/usr/bin:/home/u/.nvm/bin"[..])
    );
}

#[test]
fn no_marker_or_empty_path_is_none() {
    assert_eq!(after_marker(b"motd only"), None);
    assert_eq!(after_marker(format!("\n{MARKER}").as_bytes()), None);
}
