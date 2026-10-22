use super::*;

#[test]
fn the_environment_wins_over_the_file() {
    assert_eq!(
        pick(Some("from-env"), Some("from-file")).as_deref(),
        Some("from-env")
    );
}

#[test]
fn a_file_answers_when_nothing_is_in_the_environment() {
    assert_eq!(pick(None, Some("from-file")).as_deref(), Some("from-file"));
    assert_eq!(pick(None, None), None);
}

/// An empty or blank source is not an answer, and must fall through rather
/// than shadow the source that has one. A service manager reading an empty
/// file sets the variable to the empty string, which is how the environment
/// ends up saying "" and meaning "nothing here".
#[test]
fn a_blank_source_falls_through() {
    assert_eq!(
        pick(Some(""), Some("from-file")).as_deref(),
        Some("from-file")
    );
    assert_eq!(
        pick(Some("   \n"), Some("from-file")).as_deref(),
        Some("from-file")
    );
    assert_eq!(pick(Some(""), Some("  ")), None);
}

/// The newline every editor puts at the end of a file is not part of the
/// secret, and one sent with it is rejected by the far side with an error
/// that says nothing about a newline.
#[test]
fn the_trailing_newline_of_a_token_file_is_not_the_token() {
    assert_eq!(pick(None, Some("123:abc\n")).as_deref(), Some("123:abc"));
}

#[test]
fn only_an_owner_only_mode_is_private() {
    assert!(mode_is_private(0o600));
    assert!(mode_is_private(0o400));
    // The two that matter: the group and the rest of the machine.
    assert!(!mode_is_private(0o640));
    assert!(!mode_is_private(0o604));
    assert!(!mode_is_private(0o644));
    // The file type bits ride in the same number and say nothing about who
    // can read it.
    assert!(mode_is_private(0o100_600));
}

#[test]
fn a_secret_is_read_from_its_file_when_the_variable_is_unset() {
    let dir = std::env::temp_dir().join(format!("onehand-secret-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test.secret");
    std::fs::write(&file, "s3cret\n").unwrap();
    let var = format!("ONEHAND_TEST_SECRET_{}", std::process::id());
    assert_eq!(secret(&var, &file).as_deref(), Some("s3cret"));
    std::fs::remove_dir_all(dir).unwrap();
}
