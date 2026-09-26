//! Unattended runs: one small issue, one session, one pull request.
//!
//! Everything about a run that can be decided without a window — which issue,
//! what branch, what the prompt says, what the issue is told afterwards — and
//! the handful of `gh` calls a run makes. The app holds the rest: the timer,
//! the session, the watching.
//!
//! **`gh` is the whole API layer.** It carries the authentication, the API
//! version and the JSON, so there is no HTTP client here and no token in any
//! config. Every call is blocking and runs in the directory of the project it
//! is about, because that is how `gh` knows which repository is meant.

use serde::Deserialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// An issue a run can take.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// `"30m"`, `"2h"`, `"90s"` as a duration.
///
/// One number and one unit, nothing else. A zero is refused rather than read
/// as "continuously": an interval of nothing is a tick loop, and a timeout of
/// nothing cancels every run before its prompt goes in.
pub fn parse_every(text: &str) -> Option<Duration> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let count: u64 = text[..text.len() - unit.len_utf8()].parse().ok()?;
    let secs = match unit {
        's' => count,
        'm' => count.checked_mul(60)?,
        'h' => count.checked_mul(3600)?,
        _ => return None,
    };
    (secs > 0).then(|| Duration::from_secs(secs))
}

/// A duration the way [`parse_every`] reads one, for a sentence about it.
fn spoken(d: Duration) -> String {
    let secs = d.as_secs();
    if secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

/// The branch a run on `issue` works on: `onehand/issue-<n>-<title words>`.
///
/// Built only from lowercase ASCII letters, digits and single dashes, so it is
/// a valid branch whatever the title holds — a title of nothing but
/// punctuation leaves the number alone, and a long one is cut at a word
/// boundary's worth of characters rather than carried whole into a folder name.
pub fn branch_for(issue: &Issue) -> String {
    const WORDS_MAX: usize = 40;
    let mut words = String::new();
    for ch in issue.title.chars() {
        if ch.is_ascii_alphanumeric() {
            words.push(ch.to_ascii_lowercase());
        } else if !words.is_empty() && !words.ends_with('-') {
            words.push('-');
        }
    }
    words.truncate(WORDS_MAX);
    let words = words.trim_matches('-');
    if words.is_empty() {
        format!("onehand/issue-{}", issue.number)
    } else {
        format!("onehand/issue-{}-{words}", issue.number)
    }
}

/// `branch`, or the first of `branch-2` … `branch-9` the repository at `root`
/// does not have yet.
///
/// An issue whose label was put back after a run left work on its branch gets a
/// branch of its own this time: the old one is still checked out in the
/// worktree that run left on disk, and git refuses a second checkout of it.
pub fn free_branch_blocking(root: &Path, branch: &str) -> String {
    std::iter::once(branch.to_string())
        .chain((2..=9).map(|n| format!("{branch}-{n}")))
        .find(|name| !crate::worktree::branch_exists_blocking(root, name))
        .unwrap_or_else(|| format!("{branch}-10"))
}

/// Where every run builds, shared across runs.
///
/// The worktrees are kept on disk after a run, so a build directory inside each
/// would be gigabytes per issue with nothing that ever cleans it; one shared
/// directory also turns the second run's build from cold into incremental.
/// Under the cache directory, because a build is something that can always be
/// made again.
pub fn target_dir() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("onehand").join("unattended-target"))
}

/// The one prompt a run sends.
///
/// It does not restate the repository's conventions — the commit format, the
/// test commands, the pull-request shape. Those are in the repository's own
/// instructions, which the agent reads anyway, and a second copy here is a copy
/// that goes stale without anybody noticing.
pub fn prompt_for(issue: &Issue, branch: &str) -> String {
    format!(
        "Work GitHub issue #{number} in this repository, unattended — nobody is \
         watching this session.\n\n\
         Title: {title}\n\n\
         {body}\n\n\
         ---\n\n\
         You are on branch `{branch}`, in a worktree of its own.\n\n\
         1. Read the repository's own instructions (CLAUDE.md and what it points \
         at) and follow its conventions.\n\
         2. Run the repository's checks before committing.\n\
         3. Commit, push the branch, and open the pull request yourself with \
         `gh pr create`, referencing #{number}.\n\
         4. If the issue turns out to need a decision from a person, say so in \
         one short paragraph and stop. Do not guess.\n",
        number = issue.number,
        title = issue.title,
        body = issue.body.trim(),
    )
}

/// What the issue is told when a run takes it.
///
/// Worded to stay true if nothing follows it. A crash between the claim and
/// the outcome leaves this as the last word on the issue, so it says that a run
/// *started* — the comment's own timestamp says when — and never that one is
/// happening, which is the one sentence a crash makes false.
pub fn claim_comment(label: &str) -> String {
    format!(
        "onehand started an unattended run on this issue. If no outcome follows, \
         the run was interrupted — re-add `{label}` to retry."
    )
}

/// How a run stopped, before anybody has looked for a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The turn ended by itself. `tail` is how the agent's answer ended, which
    /// is where a question asked in prose rather than through a card would be.
    TurnEnded { tail: Option<String> },
    /// The agent parked a permission or a question nobody was there to answer.
    Asked(String),
    /// The adapter stopped answering.
    LinkLost,
    /// The run outlasted its timeout.
    TimedOut(Duration),
    /// A person acted inside the run's session.
    TakenOver,
    /// The run never got as far as a prompt.
    Failed(String),
}

impl Ending {
    /// Whether a pull request is worth looking for. A run that never started
    /// cannot have opened one.
    pub fn may_have_pr(&self) -> bool {
        !matches!(self, Self::Failed(_))
    }
}

/// The comment an ending leaves on the issue, given the pull request found on
/// `branch`, if any.
///
/// **The pull request is the verdict, whatever the ending.** An agent can open
/// it and then time out, park or lose its adapter while tidying up, and a
/// comment saying "no pull request" beside a pull request is the worst answer
/// available — so a PR found makes the comment about the PR, with why the run
/// stopped as a note under it.
pub fn report(ending: &Ending, pr: Option<&str>, branch: &str) -> String {
    if let Some(url) = pr {
        let note = match ending {
            Ending::TurnEnded { .. } => None,
            Ending::Asked(q) => Some(format!("It then stopped on a decision:\n\n{}", quoted(q))),
            Ending::LinkLost => Some("The agent then stopped answering.".to_string()),
            Ending::TimedOut(d) => Some(format!("The run then hit its {} timeout.", spoken(*d))),
            Ending::TakenOver => Some("It was then taken over by hand.".to_string()),
            Ending::Failed(why) => Some(why.clone()),
        };
        return match note {
            Some(note) => format!("onehand opened {url}.\n\n{note}"),
            None => format!("onehand opened {url}."),
        };
    }
    match ending {
        Ending::TurnEnded { tail: Some(tail) } => format!(
            "The turn ended with no pull request on `{branch}`. It ended on:\n\n{}",
            quoted(tail)
        ),
        Ending::TurnEnded { tail: None } => {
            format!("The turn ended with no pull request on `{branch}`.")
        }
        Ending::Asked(q) => format!("onehand stopped: it needs a decision.\n\n{}", quoted(q)),
        Ending::LinkLost => {
            format!("The agent stopped answering; there is no pull request on `{branch}`.")
        }
        Ending::TimedOut(d) => format!(
            "No pull request after {}; the run was cancelled. Its work is on `{branch}`.",
            spoken(*d)
        ),
        Ending::TakenOver => {
            format!("Taken over by hand; the run stopped watching `{branch}`.")
        }
        Ending::Failed(why) => format!("onehand could not start the run: {why}"),
    }
}

/// `text` as a Markdown quote, every line of it.
fn quoted(text: &str) -> String {
    text.lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run `gh` in `root` and hand back what it printed.
fn gh(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|err| format!("gh could not be run: {err}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if said.is_empty() {
            format!("gh {} failed and said nothing about why.", args[0])
        } else {
            said
        })
    }
}

/// The oldest open issue in `root`'s repository that carries `label` and was
/// opened by the user `gh` is signed in as.
///
/// **An empty label picks nothing**, without asking GitHub: the failure of
/// leaving it blank has to be "nothing runs", not "everything runs".
///
/// **Only the user's own issues.** The body goes into the prompt word for word
/// and the agent may run `git` and `gh` with the user's credentials; a label is
/// something anybody with triage rights can apply, to an issue anybody at all
/// may have written.
pub fn candidate_blocking(root: &Path, label: &str) -> Result<Option<Issue>, String> {
    if label.trim().is_empty() {
        return Ok(None);
    }
    let json = gh(
        root,
        &[
            "issue",
            "list",
            "--state",
            "open",
            "--author",
            "@me",
            "--label",
            label,
            "--limit",
            "50",
            "--json",
            "number,title,body",
        ],
    )?;
    oldest(&json)
}

/// The lowest-numbered issue in `gh issue list --json` output.
fn oldest(json: &str) -> Result<Option<Issue>, String> {
    let issues: Vec<Issue> = serde_json::from_str(json)
        .map_err(|err| format!("gh printed something unreadable: {err}"))?;
    Ok(issues.into_iter().min_by_key(|issue| issue.number))
}

/// Take `number`: remove the trigger label, then say a run started.
///
/// The label goes first because it is the whole of a run's state — once it is
/// off, nothing picks the issue again, whatever happens after.
pub fn claim_blocking(root: &Path, number: u64, label: &str) -> Result<(), String> {
    let n = number.to_string();
    gh(root, &["issue", "edit", &n, "--remove-label", label])?;
    comment_blocking(root, number, &claim_comment(label))
}

/// Leave `body` as a comment on issue `number`.
pub fn comment_blocking(root: &Path, number: u64, body: &str) -> Result<(), String> {
    gh(
        root,
        &["issue", "comment", &number.to_string(), "--body", body],
    )
    .map(drop)
}

/// The repository's default branch, as GitHub has it.
pub fn default_branch_blocking(root: &Path) -> Result<String, String> {
    let name = gh(
        root,
        &[
            "repo",
            "view",
            "--json",
            "defaultBranchRef",
            "-q",
            ".defaultBranchRef.name",
        ],
    )?;
    if name.is_empty() {
        Err("GitHub named no default branch for this repository.".to_string())
    } else {
        Ok(name)
    }
}

/// The pull request opened from `branch`, if there is one.
///
/// `--state all`, because a PR merged or closed before the run is settled is
/// still the answer to "did it open one".
pub fn pr_for_blocking(root: &Path, branch: &str) -> Result<Option<String>, String> {
    let url = gh(
        root,
        &[
            "pr", "list", "--head", branch, "--state", "all", "--json", "url", "-q", ".[0].url",
        ],
    )?;
    Ok((!url.is_empty()).then_some(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str) -> Issue {
        Issue {
            number,
            title: title.to_string(),
            body: String::new(),
        }
    }

    #[test]
    fn an_interval_is_a_number_and_a_unit() {
        assert_eq!(parse_every("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_every("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_every(" 90s "), Some(Duration::from_secs(90)));
        assert_eq!(parse_every("soon"), None);
        assert_eq!(parse_every("0m"), None);
        assert_eq!(parse_every("m"), None);
        assert_eq!(parse_every(""), None);
    }

    #[test]
    fn a_duration_is_said_the_way_it_is_written() {
        for text in ["30m", "2h", "90s"] {
            assert_eq!(spoken(parse_every(text).unwrap()), text);
        }
    }

    #[test]
    fn every_title_makes_a_valid_branch() {
        let long = "word ".repeat(60);
        for title in [
            "Fix the rail",
            "!!!???",
            "",
            &long,
            "émoji 🚀 ünïcode",
            "a.lock",
        ] {
            let branch = branch_for(&issue(7, title));
            assert!(
                crate::worktree::validate_branch(&branch).is_ok(),
                "{title:?} gave {branch:?}"
            );
            assert!(branch.starts_with("onehand/issue-7"));
            assert!(branch.len() <= "onehand/issue-7-".len() + 40);
        }
        assert_eq!(
            branch_for(&issue(3, "Fix the rail!")),
            "onehand/issue-3-fix-the-rail"
        );
        assert_eq!(branch_for(&issue(3, "!!!")), "onehand/issue-3");
    }

    #[test]
    fn the_prompt_names_the_issue_and_the_branch_and_keeps_the_body_whole() {
        let body = "Steps:\n\n```rust\nfn main() {}\n```\n\nThat is all.";
        let issue = Issue {
            body: body.to_string(),
            ..issue(42, "Crash on open")
        };
        let prompt = prompt_for(&issue, "onehand/issue-42");
        assert!(prompt.contains("#42"));
        assert!(prompt.contains("`onehand/issue-42`"));
        assert!(prompt.contains(body));
    }

    #[test]
    fn an_empty_label_picks_nothing_without_asking() {
        // A root that is not a repository at all: were `gh` asked, it would fail.
        let nowhere = std::env::temp_dir();
        assert_eq!(candidate_blocking(&nowhere, ""), Ok(None));
        assert_eq!(candidate_blocking(&nowhere, "   "), Ok(None));
    }

    #[test]
    fn the_oldest_issue_is_taken_first() {
        let json = r#"[{"number":9,"title":"b","body":""},{"number":4,"title":"a","body":"x"}]"#;
        assert_eq!(oldest(json).unwrap().map(|i| i.number), Some(4));
        assert_eq!(oldest("[]"), Ok(None));
        assert!(oldest("not json").is_err());
    }

    #[test]
    fn the_claim_stays_true_if_nothing_follows_it() {
        let said = claim_comment("auto");
        assert!(said.contains("started"));
        assert!(said.contains("re-add `auto`"));
        assert!(!said.contains("is working"));
    }

    #[test]
    fn every_ending_has_a_sentence_with_and_without_a_pr() {
        let endings = [
            Ending::TurnEnded {
                tail: Some("Should I use A or B?".into()),
            },
            Ending::TurnEnded { tail: None },
            Ending::Asked("Run rm -rf target?".into()),
            Ending::LinkLost,
            Ending::TimedOut(Duration::from_secs(2700)),
            Ending::TakenOver,
            Ending::Failed("git refused".into()),
        ];
        for ending in &endings {
            // Exhaustive on purpose: a new ending cannot be added without being
            // listed above, and so without a sentence being checked for it.
            match ending {
                Ending::TurnEnded { .. }
                | Ending::Asked(_)
                | Ending::LinkLost
                | Ending::TimedOut(_)
                | Ending::TakenOver
                | Ending::Failed(_) => {}
            }
            let without = report(ending, None, "onehand/issue-1");
            assert!(!without.is_empty());
            assert!(!without.contains("opened"), "{without}");
            let with = report(ending, Some("https://x/pull/2"), "onehand/issue-1");
            assert!(
                with.starts_with("onehand opened https://x/pull/2."),
                "{with}"
            );
        }
    }

    #[test]
    fn a_pr_found_after_a_timeout_is_still_the_verdict() {
        let said = report(
            &Ending::TimedOut(Duration::from_secs(2700)),
            Some("https://x/pull/2"),
            "b",
        );
        assert!(said.starts_with("onehand opened https://x/pull/2."));
        assert!(said.contains("45m timeout"));
    }

    #[test]
    fn a_question_in_prose_reaches_the_issue() {
        let said = report(
            &Ending::TurnEnded {
                tail: Some("Should I use A\nor B?".into()),
            },
            None,
            "b",
        );
        assert!(said.contains("> Should I use A\n> or B?"));
    }
}
