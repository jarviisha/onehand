use super::Reservations;
use std::path::Path;

#[test]
fn a_reserved_folder_refuses_a_second_open() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    let why = held.take(Path::new("/wt")).unwrap_err();
    assert!(why.contains("/wt"), "{why}");
}

#[test]
fn release_after_a_failed_open_frees_it() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    held.release(Path::new("/wt"));
    assert!(held.take(Path::new("/wt")).is_ok());
}

#[test]
fn a_bound_worker_closing_frees_its_folder() {
    let mut held = Reservations::default();
    held.take(Path::new("/wt")).unwrap();
    held.bind(Path::new("/wt"), 7);
    assert!(held.take(Path::new("/wt")).is_err());
    held.release_worker(7);
    assert!(held.take(Path::new("/wt")).is_ok());
}

#[test]
fn release_worker_leaves_other_folders() {
    let mut held = Reservations::default();
    held.take(Path::new("/a")).unwrap();
    held.bind(Path::new("/a"), 7);
    held.take(Path::new("/b")).unwrap();
    held.bind(Path::new("/b"), 8);
    held.release_worker(7);
    assert!(held.take(Path::new("/b")).is_err());
}
