use super::chip_label;
use onehand_core::host::route::Builds;
use onehand_core::workspace::HostName;

#[test]
fn the_chip_words_each_place_builds_go() {
    let host = HostName("build01".into());
    assert_eq!(
        chip_label(&Builds::Here),
        ("Builds: This machine".into(), false)
    );
    assert_eq!(
        chip_label(&Builds::On(host.clone())),
        ("Builds: build01".into(), false)
    );
    assert_eq!(
        chip_label(&Builds::HostsOff),
        ("Builds: here (hosts off)".into(), false)
    );
    assert_eq!(
        chip_label(&Builds::NotAnswering(host)),
        ("Builds: here (build01 not answering)".into(), true)
    );
}
