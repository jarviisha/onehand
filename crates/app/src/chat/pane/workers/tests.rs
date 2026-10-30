use super::tails_fit;

const HEAD: f32 = 30.;
const TAIL: f32 = 72.;
const OPEN: f32 = 300.;

#[test]
fn every_tail_is_kept_when_there_is_room() {
    // Three blocks, one open: three heads, two tails, the open minimum.
    assert!(tails_fit(
        3. * HEAD + 2. * TAIL + OPEN,
        3,
        true,
        HEAD,
        TAIL,
        OPEN
    ));
}

#[test]
fn the_tails_go_first() {
    assert!(!tails_fit(
        3. * HEAD + OPEN + 10.,
        3,
        true,
        HEAD,
        TAIL,
        OPEN
    ));
}

#[test]
fn with_none_open_only_heads_and_tails_count() {
    assert!(tails_fit(2. * HEAD + 2. * TAIL, 2, false, HEAD, TAIL, OPEN));
}
