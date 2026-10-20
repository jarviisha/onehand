use super::details::compact;
use super::{Made, pending_count};
use std::path::Path;
use std::time::Instant;

#[test]
fn a_global_change_is_pending_everywhere_and_a_project_one_only_there() {
    let before = Instant::now();
    let since = before + std::time::Duration::from_millis(1);
    let after = since + std::time::Duration::from_millis(1);
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    let made = [
        Made {
            root: a.into(),
            everywhere: false,
            at: after,
        },
        Made {
            root: a.into(),
            everywhere: true,
            at: after,
        },
        Made {
            root: a.into(),
            everywhere: false,
            at: before,
        },
    ];
    assert_eq!(pending_count(&made, a, Some(since)), 2);
    assert_eq!(pending_count(&made, b, Some(since)), 1);
    assert_eq!(pending_count(&made, a, None), 0);
}

#[test]
fn a_count_reads_at_a_glance() {
    assert_eq!(compact(7), "7");
    assert_eq!(compact(3327), "3.3k");
    assert_eq!(compact(12_000), "12k");
    assert_eq!(compact(2_450_000), "2.5M");
}
