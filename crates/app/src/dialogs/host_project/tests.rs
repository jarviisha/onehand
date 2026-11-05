use super::{
    BROWSE_CAP, HOST_CHOICES_CAP, add_refusal, browse_refusal, browse_rows, busy_body,
    clone_refusal, clone_target, host_choices,
};
use onehand_core::host::wire::HostSession;
use onehand_core::tree::TreeEntry;
use onehand_core::workspace::HostName;
use std::path::{Path, PathBuf};

#[test]
fn add_refuses_until_both_fields_name_a_folder_from_the_root() {
    assert_eq!(add_refusal("", "/srv/app"), Some("Name the host"));
    assert_eq!(add_refusal("  ", "/srv/app"), Some("Name the host"));
    assert_eq!(add_refusal("box", ""), Some("Name the folder"));
    assert_eq!(add_refusal("box", " \t"), Some("Name the folder"));
    // The host resolves the path as written, so `~` and relative paths are
    // not folders it can find.
    assert_eq!(add_refusal("box", "~/app"), Some("The folder starts at /"));
    assert_eq!(add_refusal("box", "app"), Some("The folder starts at /"));
    assert_eq!(add_refusal("box", "/srv/app"), None);
    assert_eq!(add_refusal(" me@box ", " /srv/app "), None);
}

/// The question names the sessions it would stop by when they started, as
/// the host page lists them, five of them and then how many more.
#[test]
fn the_busy_question_names_five_sessions_by_when_they_started() {
    let host = HostName("box".into());
    let now = 10_000;
    // Started 3 minutes ago, then a second apart.
    let ids = |n: usize| -> Vec<HostSession> {
        (1..=n)
            .map(|i| HostSession(format!("{}-{i}", now - 180 + i as u64)))
            .collect()
    };
    let started = "a session started 2m ago";
    assert_eq!(
        busy_body(&host, &ids(2), 0, now),
        format!(
            "box runs another build of the host service, and these sessions are mid-turn: \
             {started}, {started}. Restarting stops them."
        )
    );
    assert_eq!(
        busy_body(&host, &ids(8), 0, now),
        format!(
            "box runs another build of the host service, and these sessions are mid-turn: \
             {started}, {started}, {started}, {started}, {started} and 3 more. \
             Restarting stops them."
        )
    );
    let fresh = [HostSession(format!("{now}-1"))];
    assert_eq!(
        busy_body(&host, &fresh, 0, now),
        "box runs another build of the host service, and these sessions are mid-turn: \
         a session started just now. Restarting stops them."
    );
    assert_eq!(
        busy_body(&host, &[], 1, now),
        "box runs another build of the host service, and 1 terminal is open there. \
         Restarting closes it, and ends what runs in it."
    );
    assert_eq!(
        busy_body(&host, &fresh, 2, now),
        "box runs another build of the host service, these sessions are mid-turn: \
         a session started just now, and 2 terminals are open there. Restarting stops the \
         sessions and closes the terminals."
    );
}

fn hosts(names: &[&str]) -> Vec<HostName> {
    names.iter().map(|n| HostName((*n).into())).collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

/// The workspace's hosts lead, in root order, then the SSH config's; a host
/// named in both is offered once, where it is first.
#[test]
fn host_choices_puts_used_first_and_dedups() {
    let (choices, more) = host_choices(
        strings(&["alpha", "box", "beta", "alpha"]),
        hosts(&["box", "me@vps", "box"]),
    );
    assert_eq!(choices, strings(&["box", "me@vps", "alpha", "beta"]));
    assert_eq!(more, 0);
    assert_eq!(host_choices(Vec::new(), Vec::new()), (Vec::new(), 0));
}

#[test]
fn host_choices_caps() {
    let aliases: Vec<String> = (0..HOST_CHOICES_CAP + 7).map(|i| format!("h{i}")).collect();
    let (choices, more) = host_choices(aliases, hosts(&["used"]));
    assert_eq!(choices.len(), HOST_CHOICES_CAP);
    assert_eq!(choices[0], "used");
    assert_eq!(more, 8);
}

fn entry(at: &str, name: &str, is_dir: bool) -> TreeEntry {
    TreeEntry {
        path: Path::new(at).join(name),
        name: name.into(),
        is_dir,
    }
}

/// Folders only, in the host's order, each under the folder listed; what the
/// cap and the host's own bound left out counted together.
#[test]
fn browse_rows_keeps_folders_in_order_and_counts_what_is_left_out() {
    let at = Path::new("/srv");
    let entries = [
        entry("/srv", "b", true),
        entry("/srv", "notes.txt", false),
        entry("/srv", "a", true),
    ];
    let (dirs, hidden) = browse_rows(at, &entries, 3);
    assert_eq!(dirs, [PathBuf::from("/srv/b"), PathBuf::from("/srv/a")]);
    assert_eq!(hidden, 3);

    let many: Vec<TreeEntry> = (0..BROWSE_CAP + 5)
        .map(|i| entry("/", &format!("d{i}"), true))
        .collect();
    let (dirs, hidden) = browse_rows(Path::new("/"), &many, 2);
    assert_eq!(dirs.len(), BROWSE_CAP);
    assert_eq!(dirs[0], PathBuf::from("/d0"));
    assert_eq!(hidden, 7);
}

/// A wait outranks the attach under way, which outranks an unnamed host; the
/// row press checks the same, so *Folder* never changes while it is refused.
#[test]
fn browse_refuses_while_the_form_waits_or_attaches_or_names_no_host() {
    assert_eq!(browse_refusal("box", false, false), None);
    assert_eq!(browse_refusal("  ", false, false), Some("Name the host"));
    assert_eq!(browse_refusal("box", true, false), Some(super::ATTACHING));
    assert_eq!(browse_refusal("", true, true), Some(super::WAITING));
}

#[test]
fn clone_refuses_until_it_knows_the_repository_and_where_from_the_root() {
    let url = "https://github.com/o/repo.git";
    assert_eq!(clone_refusal(" ", url, "/srv"), Some("Name the host"));
    assert_eq!(
        clone_refusal("box", " ", "/srv"),
        Some("Name the repository")
    );
    assert_eq!(
        clone_refusal("box", "https://github.com/", "/srv"),
        Some("Cannot tell the repository's name from this URL")
    );
    assert_eq!(
        clone_refusal("box", url, " "),
        Some("Name the folder to clone into")
    );
    assert_eq!(
        clone_refusal("box", url, "~/src"),
        Some("The folder starts at /")
    );
    assert_eq!(clone_refusal("box", url, "/srv"), None);
    assert_eq!(
        clone_refusal(" box ", " git@github.com:o/repo.git ", " /srv "),
        None
    );
}

#[test]
fn clone_target_joins_the_name() {
    assert_eq!(
        clone_target("git@github.com:o/repo.git", Path::new("/srv")),
        Some(PathBuf::from("/srv/repo"))
    );
}

#[test]
fn clone_target_none_for_a_bare_host() {
    assert_eq!(clone_target("https://github.com", Path::new("/srv")), None);
}
