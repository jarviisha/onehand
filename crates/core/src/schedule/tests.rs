use super::*;
use chrono::FixedOffset;

/// The tests' time zone: seven hours east, so a local day is not a UTC day.
fn tz() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).unwrap()
}

/// Epoch seconds of a local time in [`tz`].
fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> u64 {
    tz().with_ymd_and_hms(y, mo, d, h, mi, s)
        .unwrap()
        .timestamp() as u64
}

fn local(secs: u64) -> chrono::DateTime<FixedOffset> {
    tz().timestamp_opt(secs as i64, 0).unwrap()
}

fn daily_at_nine() -> Schedule {
    Schedule {
        id: "s1".into(),
        name: "Nightly code review".into(),
        project: PathBuf::from("/p"),
        on: true,
        starts: Starts::Prompt {
            prompt: "review".into(),
        },
        agent: None,
        mode: None,
        options: BTreeMap::new(),
        tools: Vec::new(),
        labels: Vec::new(),
        when: "0 9 * * *".into(),
        missed: Missed::RunOnce,
        overlap: Overlap::Skip,
        on_since: at(2026, 10, 1, 8, 59, 0),
        last_fired: None,
        said: None,
    }
}

#[test]
fn every_field_form_parses() {
    let cron = parse("*/15 9-17 * * 1-5").unwrap();
    // Monday 2026-10-19, 09:15.
    assert!(cron.matches(&local(at(2026, 10, 19, 9, 15, 0))));
    assert!(!cron.matches(&local(at(2026, 10, 19, 9, 16, 0))));
    assert!(!cron.matches(&local(at(2026, 10, 19, 18, 0, 0))));
    // Saturday.
    assert!(!cron.matches(&local(at(2026, 10, 24, 9, 15, 0))));
    let cron = parse("0,30 * * * *").unwrap();
    assert!(cron.matches(&local(at(2026, 10, 19, 3, 30, 0))));
    let cron = parse("0 0 1 1 *").unwrap();
    assert!(cron.matches(&local(at(2027, 1, 1, 0, 0, 0))));
    // 7 is Sunday as well as 0; 2026-10-25 is a Sunday.
    let cron = parse("0 9 * * 7").unwrap();
    assert!(cron.matches(&local(at(2026, 10, 25, 9, 0, 0))));
    let cron = parse("0 8-18/5 * * *").unwrap();
    assert!(cron.matches(&local(at(2026, 10, 25, 13, 0, 0))));
    assert!(!cron.matches(&local(at(2026, 10, 25, 14, 0, 0))));
}

#[test]
fn a_bad_field_is_named() {
    assert!(parse("60 * * * *").unwrap_err().contains("minute"));
    assert!(parse("* 24 * * *").unwrap_err().contains("hour"));
    assert!(parse("* * * *").unwrap_err().contains("5 fields"));
    assert!(parse("*/0 * * * *").unwrap_err().contains("step"));
    assert!(parse("* * 0 * *").unwrap_err().contains("day of month"));
    assert!(parse("* * * 13 *").unwrap_err().contains("month"));
    assert!(parse("* * * * 8").unwrap_err().contains("day of week"));
    assert!(parse("5-2 * * * *").unwrap_err().contains("minute"));
    assert!(parse("x * * * *").unwrap_err().contains("minute"));
}

#[test]
fn day_of_month_and_weekday_restricted_match_either() {
    let cron = parse("0 9 1 * 1").unwrap();
    // The 1st, a Thursday.
    assert!(cron.matches(&local(at(2026, 10, 1, 9, 0, 0))));
    // A Monday that is not the 1st.
    assert!(cron.matches(&local(at(2026, 10, 19, 9, 0, 0))));
    // Neither.
    assert!(!cron.matches(&local(at(2026, 10, 20, 9, 0, 0))));
    // Only one restricted: both must hold.
    let cron = parse("0 9 1 * *").unwrap();
    assert!(!cron.matches(&local(at(2026, 10, 19, 9, 0, 0))));
}

#[test]
fn quick_choices_write_and_read_back() {
    let all = [
        Quick::EveryHours {
            hours: 3,
            minute: 5,
        },
        Quick::Daily { hour: 9, minute: 0 },
        Quick::Weekdays {
            hour: 8,
            minute: 30,
        },
        Quick::Weekly {
            weekday: 1,
            hour: 9,
            minute: 0,
        },
    ];
    for quick in all {
        let expr = quick.expr();
        assert!(parse(&expr).is_ok(), "{expr}");
        assert_eq!(Quick::read(&expr), Some(quick), "{expr}");
    }
    assert_eq!(Quick::Daily { hour: 9, minute: 0 }.expr(), "0 9 * * *");
    assert_eq!(Quick::Weekdays { hour: 9, minute: 0 }.expr(), "0 9 * * 1-5");
    assert_eq!(Quick::read("0 9 1 * *"), None);
    assert_eq!(Quick::read("0 9 * * 1,3"), None);
}

#[test]
fn fires_once_for_one_due_time() {
    let mut s = daily_at_nine();
    let opened = at(2026, 10, 1, 8, 0, 0);
    let now = at(2026, 10, 1, 9, 0, 30);
    assert_eq!(
        decide(&s, now, opened, false, &tz()),
        Act::Start { missed: false }
    );
    s.last_fired = Some(now);
    assert_eq!(
        decide(&s, at(2026, 10, 1, 9, 0, 59), opened, false, &tz()),
        Act::Wait
    );
    // Not before nine.
    s.last_fired = None;
    assert_eq!(
        decide(&s, at(2026, 10, 1, 8, 59, 59), opened, false, &tz()),
        Act::Wait
    );
}

#[test]
fn a_week_missed_is_one_start() {
    let mut s = daily_at_nine();
    s.last_fired = Some(at(2026, 10, 1, 9, 0, 10));
    let opened = at(2026, 10, 8, 14, 0, 0);
    let now = at(2026, 10, 8, 14, 0, 5);
    assert_eq!(
        decide(&s, now, opened, false, &tz()),
        Act::Start { missed: true }
    );
    s.last_fired = Some(now);
    assert_eq!(decide(&s, now + 60, opened, false, &tz()), Act::Wait);
}

#[test]
fn a_missed_time_set_to_skip_is_skipped() {
    let mut s = daily_at_nine();
    s.missed = Missed::Skip;
    let opened = at(2026, 10, 2, 14, 0, 0);
    let act = decide(&s, opened + 5, opened, false, &tz());
    assert_eq!(act, Act::Skip("missed while onehand was closed".into()));
    // Due while it was open is not missed, whatever the setting.
    let opened = at(2026, 10, 2, 8, 0, 0);
    let act = decide(&s, at(2026, 10, 2, 9, 0, 1), opened, false, &tz());
    assert_eq!(act, Act::Start { missed: false });
}

#[test]
fn nothing_before_it_was_switched_on() {
    let mut s = daily_at_nine();
    s.on_since = at(2026, 10, 5, 10, 0, 0);
    let opened = at(2026, 10, 1, 8, 0, 0);
    // Due on the 1st to the 5th at nine, all before it was switched on.
    assert_eq!(
        decide(&s, at(2026, 10, 5, 10, 0, 30), opened, false, &tz()),
        Act::Wait
    );
    assert_eq!(
        decide(&s, at(2026, 10, 6, 9, 0, 0), opened, false, &tz()),
        Act::Start { missed: false }
    );
    s.on = false;
    assert_eq!(
        decide(&s, at(2026, 10, 6, 9, 0, 0), opened, false, &tz()),
        Act::Wait
    );
}

#[test]
fn a_busy_schedule_skips_or_queues_as_set() {
    let mut s = daily_at_nine();
    let opened = at(2026, 10, 1, 8, 0, 0);
    let now = at(2026, 10, 1, 9, 0, 1);
    assert_eq!(
        decide(&s, now, opened, true, &tz()),
        Act::Skip("the last run was still going".into())
    );
    s.overlap = Overlap::Queue;
    assert_eq!(decide(&s, now, opened, true, &tz()), Act::Queue);
}

#[test]
fn an_expression_that_never_matches_is_bounded() {
    let mut s = daily_at_nine();
    s.when = "0 9 31 2 *".into();
    s.on_since = 0;
    let began = std::time::Instant::now();
    let now = at(2026, 10, 1, 9, 0, 0);
    assert_eq!(decide(&s, now, now, false, &tz()), Act::Wait);
    assert!(next_times(&s.when, now, 3, &tz()).is_empty());
    assert!(began.elapsed() < std::time::Duration::from_secs(1));
    // One that does not parse waits too.
    s.when = "nonsense".into();
    assert_eq!(decide(&s, now, now, false, &tz()), Act::Wait);
}

#[test]
fn next_times_lists_the_coming_runs() {
    let after = at(2026, 10, 1, 8, 0, 0);
    let next = next_times("0 9 * * *", after, 2, &tz());
    assert_eq!(
        next,
        vec![at(2026, 10, 1, 9, 0, 0), at(2026, 10, 2, 9, 0, 0)]
    );
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onehand-schedule-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn schedules_round_trip_through_the_file() {
    let dir = scratch("round");
    let file = dir.join("schedules.toml");
    let mut other = daily_at_nine();
    other.id = "s2".into();
    other.starts = Starts::Workflow {
        workflow: "builtin:checkout".into(),
        title: "Tidy".into(),
        body: String::new(),
    };
    other.options = BTreeMap::from([("model".to_string(), "opus".to_string())]);
    other.tools = vec!["list_issues".into(), "create_issue".into()];
    other.labels = vec!["auto".into()];
    other.overlap = Overlap::Queue;
    other.last_fired = Some(5);
    let all = vec![daily_at_nine(), other];
    save_blocking(&file, &all).unwrap();
    assert_eq!(load_blocking(&file).unwrap(), all);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_no_schedules() {
    let dir = scratch("missing");
    assert_eq!(
        load_blocking(&dir.join("schedules.toml")).unwrap(),
        Vec::new()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_does_not_parse_says_where() {
    let dir = scratch("bad");
    let file = dir.join("schedules.toml");
    std::fs::write(
        &file,
        "[[schedule]]\nid = \"a\"\nname = \"n\"\nproject = \"/p\"\nwhen = \"* * * * *\"\n\
         [schedule.starts]\nkind = \"teleport\"\n",
    )
    .unwrap();
    let why = load_blocking(&file).unwrap_err();
    assert!(why.contains("schedules.toml"), "{why}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn defaults_fill_what_the_file_leaves_out() {
    let dir = scratch("defaults");
    let file = dir.join("schedules.toml");
    std::fs::write(
        &file,
        "[[schedule]]\nid = \"a\"\nname = \"n\"\nproject = \"/p\"\nwhen = \"0 9 * * *\"\n\
         [schedule.starts]\nkind = \"prompt\"\nprompt = \"hi\"\n",
    )
    .unwrap();
    let all = load_blocking(&file).unwrap();
    let s = &all[0];
    assert!(!s.on);
    assert_eq!(s.missed, Missed::RunOnce);
    assert_eq!(s.overlap, Overlap::Skip);
    assert!(s.tools.is_empty() && s.labels.is_empty() && s.options.is_empty());
    assert_eq!(s.last_fired, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_step_on_one_number_runs_from_it_to_the_end() {
    let cron = parse("5/15 * * * *").unwrap();
    for minute in [5, 20, 35, 50] {
        assert!(
            cron.matches(&local(at(2026, 10, 1, 9, minute, 0))),
            "{minute}"
        );
    }
    assert!(!cron.matches(&local(at(2026, 10, 1, 9, 6, 0))));
}

#[test]
fn a_starred_step_counts_as_a_star_for_the_either_rule() {
    // Odd days that are Mondays, as cron reads it: 2026-10-19 is a Monday.
    let cron = parse("0 9 */2 * 1").unwrap();
    assert!(cron.matches(&local(at(2026, 10, 19, 9, 0, 0))));
    // An odd day that is not a Monday, and a Monday that is not odd.
    assert!(!cron.matches(&local(at(2026, 10, 21, 9, 0, 0))));
    assert!(!cron.matches(&local(at(2026, 10, 26, 9, 0, 0))));
}

#[test]
fn a_schedule_switched_on_by_hand_in_the_file_catches_nothing_up() {
    let mut s = daily_at_nine();
    s.on_since = 0;
    let opened = at(2026, 10, 1, 15, 0, 0);
    assert_eq!(decide(&s, opened + 30, opened, false, &tz()), Act::Wait);
    assert_eq!(
        decide(&s, at(2026, 10, 2, 9, 0, 0), opened, false, &tz()),
        Act::Start { missed: false }
    );
}

#[test]
fn a_rare_expression_is_cheap_in_the_machines_own_zone() {
    let mut s = daily_at_nine();
    s.when = "0 9 31 2 *".into();
    s.on_since = 1;
    let now = at(2026, 10, 1, 9, 0, 0);
    let began = std::time::Instant::now();
    for _ in 0..10 {
        assert_eq!(decide(&s, now, now, false, &chrono::Local), Act::Wait);
        assert!(next_times("0 0 1 1 *", now, 3, &chrono::Local).len() <= 1);
    }
    assert!(
        began.elapsed() < std::time::Duration::from_millis(200),
        "{:?}",
        began.elapsed()
    );
}

#[test]
fn the_prompt_template_validates() {
    let template = prompt_template();
    assert_eq!(template.name, "Prompt");
    assert!(crate::workflow::validate(&template).is_empty());
    assert_eq!(template.place, crate::workflow::Place::Checkout);
    let s = daily_at_nine();
    let brief = brief(&s);
    assert_eq!(brief.title, "Nightly code review");
    assert_eq!(brief.body, "review");
}

#[test]
fn fill_keeps_a_steps_own_settings() {
    use crate::workflow::{StepKind, StepSpec};
    let mut template = prompt_template();
    template.steps.push(StepSpec {
        id: "own".into(),
        label: "Own".into(),
        kind: StepKind::Agent {
            prompt: "{brief}".into(),
            gates: Vec::new(),
            keep_answer: false,
            mode: Some("plan".into()),
            options: BTreeMap::from([("model".to_string(), "cheap".to_string())]),
        },
    });
    let options = BTreeMap::from([
        ("model".to_string(), "strong".to_string()),
        ("effort".to_string(), "high".to_string()),
    ]);
    let filled = fill(template, Some("acceptEdits"), &options);
    let settings: Vec<_> = filled
        .steps
        .iter()
        .map(|step| match &step.kind {
            StepKind::Agent { mode, options, .. } => (mode.clone(), options.clone()),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(settings[0].0.as_deref(), Some("acceptEdits"));
    assert_eq!(settings[0].1, options);
    assert_eq!(settings[1].0.as_deref(), Some("plan"));
    assert_eq!(settings[1].1["model"], "cheap");
    assert_eq!(settings[1].1["effort"], "high");
}

#[test]
fn quick_choices_read_in_words() {
    assert_eq!(when_said("0 9 * * *"), "every day at 09:00");
    assert_eq!(when_said("5 */3 * * *"), "every 3 hours at :05");
    assert_eq!(when_said("30 8 * * 1-5"), "weekdays at 08:30");
    assert_eq!(when_said("0 9 * * 1"), "Mondays at 09:00");
    assert_eq!(when_said("0 9 * * 0"), "Sundays at 09:00");
    assert_eq!(when_said("0 9 1 * *"), "0 9 1 * *");
}

#[test]
fn a_bad_expression_says_why() {
    let said = when_said("0 25 * * *");
    assert!(said.starts_with("0 25 * * * does not parse"), "{said}");
    assert!(said.contains("hour"), "{said}");
}

#[test]
fn the_next_time_reads_near_or_far() {
    let now = at(2026, 10, 19, 8, 0, 0); // a Monday
    assert_eq!(
        next_said(at(2026, 10, 19, 9, 0, 0), now, &tz()),
        "today 09:00"
    );
    assert_eq!(
        next_said(at(2026, 10, 20, 9, 0, 0), now, &tz()),
        "tomorrow 09:00"
    );
    assert_eq!(
        next_said(at(2026, 10, 23, 9, 30, 0), now, &tz()),
        "Fri 09:30"
    );
    assert_eq!(
        next_said(at(2026, 10, 30, 9, 0, 0), now, &tz()),
        "Oct 30 09:00"
    );
}

#[test]
fn a_blank_schedule_has_every_problem() {
    let mut s = daily_at_nine();
    s.name = "  ".into();
    s.project = PathBuf::new();
    s.starts = Starts::Prompt { prompt: " ".into() };
    s.when = "61 * * * *".into();
    let said = problems(&s);
    assert_eq!(said.len(), 4, "{said:?}");
    assert!(said[0].contains("Name"));
    assert!(said[1].contains("project"));
    assert!(said[2].contains("ask"));
    assert!(said[3].starts_with("When:"));
    s.starts = Starts::Workflow {
        workflow: String::new(),
        title: String::new(),
        body: String::new(),
    };
    let said = problems(&s);
    assert!(said.iter().any(|p| p.contains("workflow")), "{said:?}");
    assert!(said.iter().any(|p| p.contains("title")), "{said:?}");
}

#[test]
fn a_whole_schedule_has_none() {
    assert!(problems(&daily_at_nine()).is_empty());
}
