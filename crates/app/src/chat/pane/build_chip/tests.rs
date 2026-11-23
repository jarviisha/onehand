use super::chip_label;
use onehand_core::host::route::drive::Standing;
use onehand_core::workspace::HostName;

#[test]
fn the_chip_says_where_builds_go_now() {
    let host = HostName("build01".into());
    let label = |picked, on, standing| chip_label(picked, on, standing);
    assert_eq!(
        label(None, true, None),
        ("Builds: This machine".into(), false)
    );
    assert_eq!(
        label(None, false, None),
        ("Builds: This machine".into(), false)
    );
    assert_eq!(
        label(Some(&host), false, Some(Standing::NotAnswering)),
        ("Builds: here (hosts off)".into(), false)
    );
    assert_eq!(
        label(Some(&host), true, Some(Standing::NotAnswering)),
        ("Builds: here (build01 not answering)".into(), true)
    );
    assert_eq!(
        label(Some(&host), true, None),
        ("Builds: build01".into(), false)
    );
    assert_eq!(
        label(Some(&host), true, Some(Standing::Reached)),
        ("Builds: build01".into(), false)
    );
}
