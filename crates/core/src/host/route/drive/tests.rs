use super::*;

fn dir(n: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("onehand-route-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn on_by_default() {
    assert!(RouteDrive::load(&dir("default")).hosts_on());
}

#[test]
fn off_when_the_flag_file_is_there() {
    let d = dir("flag");
    std::fs::create_dir_all(d.join("builds")).unwrap();
    std::fs::write(d.join("builds/hosts-off"), b"").unwrap();
    assert!(!RouteDrive::load(&d).hosts_on());
}

#[test]
fn set_hosts_on_answers_a_write_only_on_change() {
    let mut r = RouteDrive::load(&dir("set"));
    assert!(r.set_hosts_on(true).is_empty());
    assert_eq!(r.set_hosts_on(false), vec![RouteEffect::WriteSwitch(false)]);
    assert!(r.set_hosts_on(false).is_empty());
    assert!(!r.hosts_on());
}

#[test]
fn write_switch_round_trips() {
    let d = dir("trip");
    write_switch_blocking(&d, false).unwrap();
    assert!(!RouteDrive::load(&d).hosts_on());
    write_switch_blocking(&d, true).unwrap();
    assert!(!d.join("builds/hosts-off").exists());
    write_switch_blocking(&d, true).unwrap();
}

#[test]
fn standing_follows_the_last_run() {
    let mut r = RouteDrive::load(&dir("standing"));
    let h = HostName("h".into());
    assert_eq!(r.standing(&h), None);
    r.reached(&h, false);
    assert_eq!(r.standing(&h), Some(Standing::NotAnswering));
    r.reached(&h, true);
    assert_eq!(r.standing(&h), Some(Standing::Reached));
}

#[test]
fn a_host_down_is_not_asked_again_for_a_while() {
    let mut r = RouteDrive::load(&dir("down"));
    let h = HostName("h".into());
    let t = std::time::Instant::now();
    let later = |s| t + std::time::Duration::from_secs(s);
    assert!(!r.recently_down(&h, t));
    assert!(r.reached_at(&h, false, t), "a change");
    assert!(r.recently_down(&h, later(29)));
    assert!(!r.recently_down(&h, later(30)));
    assert!(!r.reached_at(&h, false, later(31)), "no change");
    assert!(r.recently_down(&h, later(40)));
    assert!(r.reached_at(&h, true, later(70)));
    assert!(!r.recently_down(&h, later(71)));
}

/// A failed write puts the switch back as the file still has it, writing
/// nothing; unless the switch was turned again meanwhile.
#[test]
fn a_failed_write_puts_the_switch_back() {
    let mut r = RouteDrive::load(&dir("failed"));
    let _ = r.set_hosts_on(false);
    assert!(r.write_failed(false).is_empty());
    assert!(r.hosts_on());
    let _ = r.set_hosts_on(false);
    let _ = r.set_hosts_on(true);
    assert!(r.write_failed(false).is_empty());
    assert!(r.hosts_on());
    let _ = r.set_hosts_on(false);
    assert!(r.write_failed(true).is_empty());
    assert!(!r.hosts_on());
}
