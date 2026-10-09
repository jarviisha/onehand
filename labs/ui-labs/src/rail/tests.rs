use super::model::{Filter, Status, attention_count, badge, seed, toggle_attention, visible};

#[test]
fn a_filter_keeps_what_it_says() {
    let s = seed();
    assert_eq!(visible(&s, Filter::All, ""), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(visible(&s, Filter::ByProject, ""), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(visible(&s, Filter::NeedsAttention, ""), vec![0, 2]);
    // Youngest first: 1m, 4m, 12m, 25m, 90m, two days.
    assert_eq!(visible(&s, Filter::Newest, ""), vec![1, 0, 5, 2, 4, 3]);
}

#[test]
fn the_query_matches_a_title_or_a_project_ignoring_case() {
    let s = seed();
    assert_eq!(visible(&s, Filter::All, "  RETRY "), vec![1, 3]);
    assert_eq!(visible(&s, Filter::All, "billing"), vec![4, 5]);
    assert_eq!(
        visible(&s, Filter::NeedsAttention, "retry"),
        Vec::<usize>::new()
    );
}

#[test]
fn attention_counts_input_and_failures() {
    let mut s = seed();
    assert_eq!(attention_count(&s), 2);
    s[1].status = Status::Failed;
    assert_eq!(attention_count(&s), 3);
}

#[test]
fn the_meta_line_keeps_one_order_and_never_trades_time_for_the_diff() {
    let s = seed();
    // Running with a diff still says its time; the diff is drawn apart.
    assert_eq!(s[1].meta("claude"), "Running · claude · 1m");
    assert_eq!(s[0].meta("claude"), "Needs input · claude · waiting 4m");
    assert_eq!(s[2].meta("codex"), "Failed · codex · 25m");
    // Idle, no diff.
    assert_eq!(s[3].meta("claude"), "Idle · claude · 2d");
}

#[test]
fn a_folded_project_shows_its_most_urgent_state() {
    let mut s = seed();
    assert_eq!(badge(&s, 0), Some(Status::Failed));
    s[2].status = Status::Idle;
    assert_eq!(badge(&s, 0), Some(Status::NeedsInput));
    // Done and running only: the run shows.
    assert_eq!(badge(&s, 1), Some(Status::Running));
    s[5].status = Status::Idle;
    // Done is not waiting on anyone and nothing runs.
    assert_eq!(badge(&s, 1), None);
    assert_eq!(badge(&s, 2), None);
}

#[test]
fn the_attention_chip_toggles_back_to_the_filter_it_replaced() {
    assert_eq!(
        toggle_attention(Filter::Newest, Filter::Newest),
        Filter::NeedsAttention
    );
    assert_eq!(
        toggle_attention(Filter::NeedsAttention, Filter::Newest),
        Filter::Newest
    );
}
