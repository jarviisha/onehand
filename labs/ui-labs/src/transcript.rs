//! The canned conversation the chat opens on: three turns that between them
//! hold every block a transcript can, the last one still running until Stop,
//! as the composer under it says. How each block draws is in `blocks`.
use super::*;

mod blocks;
use blocks::{Detail, Kind, Line, State, Tool};

// ---- turn 1: the flaky test --------------------------------------------------

const THOUGHT_1: &str = "The failure only shows on slow runs, so the test is racing something. The backoff sleeps on `Instant::now()`; three attempts at 200, 400 and 800 ms plus the work between them land close to the 5 s timeout.";

const LOOK: [Tool; 5] = [
    Tool {
        id: "t1-read-test",
        kind: Kind::Read,
        verb: "Read",
        object: "tests/flaky.rs",
        state: State::Done,
        detail: Detail::Lines(&[
            "#[test]",
            "fn backoff_caps() {",
            "    let start = Instant::now();",
            "    retry(|| flaky_call(), Backoff::default());",
            "    assert!(start.elapsed() < Duration::from_secs(5));",
            "}",
        ]),
    },
    Tool {
        id: "t1-read-backoff",
        kind: Kind::Read,
        verb: "Read",
        object: "src/backoff.rs",
        state: State::Done,
        detail: Detail::None,
    },
    Tool {
        id: "t1-search",
        kind: Kind::Search,
        verb: "Searched",
        object: "Instant::now",
        state: State::Done,
        detail: Detail::Lines(&[
            "src/backoff.rs:42    let start = Instant::now();",
            "tests/flaky.rs:3     let start = Instant::now();",
        ]),
    },
    Tool {
        id: "t1-build",
        kind: Kind::Run,
        verb: "Ran",
        object: "cargo build -p retry",
        state: State::Done,
        detail: Detail::Output(&[
            "   Compiling retry v0.4.1 (/work/retry)",
            "    Finished `dev` profile [optimized + debuginfo] in 4.1s",
        ]),
    },
    Tool {
        id: "t1-test",
        kind: Kind::Run,
        verb: "Ran",
        object: "cargo test -p retry -- flaky",
        state: State::Failed("exit 101"),
        detail: Detail::Output(&[
            "running 12 tests",
            "test backoff_doubles ... ok",
            "test backoff_gives_up ... ok",
            "test backoff_jitter_off ... ok",
            "test backoff_reads_clock ... ok",
            "test backoff_caps ... FAILED",
            "thread 'backoff_caps' panicked at 'elapsed 5.2s > 5s'",
            "test result: FAILED. 11 passed; 1 failed",
        ]),
    },
];

const FIX: [Tool; 3] = [
    Tool {
        id: "t1-edit",
        kind: Kind::Edit,
        verb: "Edited",
        object: "src/backoff.rs",
        state: State::Done,
        detail: Detail::Diff(
            41,
            &[
                (Line::Same, "pub fn wait(&self, attempt: u32) -> Duration {"),
                (Line::Removed, "    let start = Instant::now();"),
                (Line::Removed, "    let elapsed = start.elapsed();"),
                (Line::Added, "    let start = self.clock.now();"),
                (Line::Added, "    let elapsed = self.clock.now() - start;"),
                (Line::Added, "    debug_assert!(attempt <= self.max);"),
                (Line::Same, "    self.base * 2u32.pow(attempt) - elapsed"),
                (Line::Same, "}"),
            ],
        ),
    },
    Tool {
        id: "t1-create",
        kind: Kind::Create,
        verb: "Created",
        object: "src/clock.rs",
        state: State::Done,
        detail: Detail::Diff(
            1,
            &[
                (Line::Added, "pub trait Clock {"),
                (Line::Added, "    fn now(&self) -> Instant;"),
                (Line::Added, "}"),
            ],
        ),
    },
    Tool {
        id: "t1-retest",
        kind: Kind::Run,
        verb: "Ran",
        object: "cargo test -p retry -- flaky",
        state: State::Done,
        detail: Detail::Output(&[
            "running 12 tests",
            "test backoff_doubles ... ok",
            "test backoff_gives_up ... ok",
            "test backoff_jitter_off ... ok",
            "test backoff_reads_clock ... ok",
            "test backoff_caps ... ok",
            "test retry_stops_on_success ... ok",
            "test result: ok. 12 passed; 0 failed",
        ]),
    },
];

/// The turn's last words: what its footer copies.
const CLOSING_1: &str = "It passed 200 runs in a row. I did not push: you denied the force push, and the branch is ahead of `origin/main` by one commit.";

const SNIPPET: &str = "let clock = FakeClock::new();\nlet backoff = Backoff::new(&clock);\nclock.advance(Duration::from_secs(1));";

// ---- turn 2: the docs, cut short ---------------------------------------------

const TIDY: [Tool; 4] = [
    Tool {
        id: "t2-fetch",
        kind: Kind::Fetch,
        verb: "Fetched",
        object: "docs.rs/tokio/latest/tokio/time",
        state: State::Done,
        detail: Detail::None,
    },
    Tool {
        id: "t2-move",
        kind: Kind::Move,
        verb: "Moved",
        object: "docs/retry.md → docs/retry/backoff.md",
        state: State::Done,
        detail: Detail::None,
    },
    Tool {
        id: "t2-delete",
        kind: Kind::Delete,
        verb: "Deleted",
        object: "src/old_clock.rs",
        state: State::Done,
        detail: Detail::None,
    },
    Tool {
        id: "t2-lock",
        kind: Kind::Edit,
        verb: "Edited",
        object: "Cargo.lock",
        state: State::Done,
        detail: Detail::Large(
            300,
            212,
            &[
                (Line::Same, "[[package]]"),
                (Line::Same, "name = \"retry\""),
                (Line::Removed, "version = \"0.4.0\""),
                (Line::Added, "version = \"0.4.1\""),
                (Line::Same, "dependencies = ["),
                (Line::Removed, " \"tokio 1.38.0\","),
                (Line::Added, " \"tokio 1.41.1\","),
                (Line::Same, "]"),
            ],
        ),
    },
];

// ---- turn 3: running -----------------------------------------------------------

const SUITE: [Tool; 2] = [
    Tool {
        id: "t3-read",
        kind: Kind::Read,
        verb: "Read",
        object: "Cargo.toml",
        state: State::Done,
        detail: Detail::None,
    },
    Tool {
        id: "t3-test",
        kind: Kind::Run,
        verb: "Running",
        object: "cargo test --workspace",
        state: State::Running,
        detail: Detail::Output(&[
            "   Compiling retry v0.4.1 (/work/retry)",
            "   Compiling retry-cli v0.4.1 (/work/retry-cli)",
            "    Finished `test` profile [unoptimized + debuginfo] in 11.8s",
            "     Running unittests src/lib.rs (target/debug/deps/retry-3f2a)",
            "running 12 tests",
            "test backoff_doubles ... ok",
            "test backoff_gives_up ... ok",
            "test backoff_caps ... ok",
            "     Running unittests src/main.rs (target/debug/deps/retry_cli-9c1e)",
            "running 4 tests",
        ]),
    },
];

const WHY_1: &str = "## Why it fails

Two things meet in `backoff_caps`:

- `Backoff::wait` reads `Instant::now()`, so the test waits for real.
- The test allows 5 s, and three attempts take 4.6 s before any work.";

const ANSWER_2: &str = "# Configuring the backoff

Every retry waits twice as long as the one before it. Three settings in `retry.toml` change that; see [the tokio time docs](https://docs.rs/tokio/latest/tokio/time) for how the clock is read.

| Setting | Default | What it does |
|---|---|---|
| `base_ms` | 200 | The first wait |
| `max_attempts` | 5 | Attempts before giving up |
| `jitter` | off | Spreads retries from many clients |

### Changing it

1. Edit `retry.toml` at the project root.
2. Restart the service; the file is read once, at start.
3. Check the first wait in the log: `retry: waiting 200ms`.

> A cap on the wait is not configurable yet; the longest wait is `base_ms` × 2⁴.";

impl Labs {
    /// The canned turns, top to bottom, before anything typed in the lab.
    pub(super) fn transcript(
        &self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let mono = cx.theme().mono_font_family.clone();
        // Turn 3 runs until Stop; then what was running reads as stopped.
        let running = self.live.canned_running;
        let suite = SUITE.map(|t| match t.state {
            State::Running if !running => Tool {
                verb: "Ran",
                state: State::Stopped,
                ..t
            },
            _ => t,
        });
        let done_1 = format!(
            "Done. The test now drives a fake clock, so it never sleeps:\n\n```rust\n{SNIPPET}\n```\n\n{CLOSING_1}"
        );
        vec![
            // Turn 1: every block a working turn has.
            self.bubble(p, "The retry test fails about one run in five. Find out why and fix it.", &[], cx)
                .into_any_element(),
            self.thought(p, "t1-thought", Some(4), THOUGHT_1, window, cx).into_any_element(),
            self.md(p, "t1-intro", "I'll read the test and the retry function first, then run it to see the failure.", window, cx)
                .into_any_element(),
            self.activity(p, mono.clone(), "t1-look", Some("14s"), &LOOK, cx).into_any_element(),
            self.md(p, "t1-why", WHY_1, window, cx).into_any_element(),
            self.plan(
                p,
                "t1-plan",
                &[
                    (State::Done, "Read the test and the backoff"),
                    (State::Done, "Inject a clock into `Backoff`"),
                    (State::Done, "Run the test 200 times"),
                ],
                &["Note the change in `docs/retry.md`"],
                cx,
            )
            .into_any_element(),
            self.settled(p, Some(mono.clone()), "Allowed", "cargo test -p retry -- flaky", "always", false)
                .into_any_element(),
            self.activity(p, mono.clone(), "t1-fix", Some("38s"), &FIX, cx).into_any_element(),
            self.settled(p, None, "Asked", "Run the test 200 times or 50?", "200", false)
                .into_any_element(),
            self.settled(p, Some(mono.clone()), "Denied", "git push --force", "deny", true)
                .into_any_element(),
            self.md(p, "t1-done", done_1, window, cx).into_any_element(),
            self.footer(p, "t1-copy", "1m 12s", CLOSING_1, cx).into_any_element(),
            // Turn 2: the reference prose of a longer answer, then a failure.
            self.bubble(p, "Write up how the backoff is configured, and tidy the old clock away.", &["retry.toml", "ci-log.txt"], cx)
                .into_any_element(),
            self.thought(p, "t2-thought", Some(2), "The settings live in `retry.toml`; the CI log shows the defaults in use. A table of the three settings reads faster than prose.", window, cx)
                .into_any_element(),
            self.md(p, "t2-answer", ANSWER_2, window, cx).into_any_element(),
            self.activity(p, mono.clone(), "t2-tidy", Some("9s"), &TIDY, cx).into_any_element(),
            self.notice(p, "Context compacted · 38k tokens freed").into_any_element(),
            self.error(p, "The agent stopped: the rate limit was reached. Try again in 30 s.", cx)
                .into_any_element(),
            self.notice(p, "Reconnected to Claude").into_any_element(),
            // Turn 3: still running.
            self.bubble(p, "Try again, and run the whole suite this time.", &[], cx)
                .into_any_element(),
            self.notice(p, "Workflow Verify · step 2 of 3").into_any_element(),
            self.thought(p, "t3-thought", (!running).then_some(3), "The workspace has two crates; run them together so the CLI's tests see the new clock.", window, cx)
                .into_any_element(),
            self.activity(p, mono, "t3-suite", (!running).then_some("21s"), &suite, cx)
                .into_any_element(),
        ]
    }
}
