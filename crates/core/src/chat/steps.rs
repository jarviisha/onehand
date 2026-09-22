//! What one step of a run says on its own line, and what a run of them says as
//! a whole.
//!
//! **A child line is a footnote, not a second copy of its parent.** The row
//! above it has already named the verb and the kind of work; what is left for a
//! child to say is which one of the seven it was — and the only thing that
//! answers that is what the step was pointed at and what it did there. So a
//! line here is `target · action`, and everything a reader could not tell two
//! steps apart by is dropped: the directory change, the variable that was set
//! first, the prefix all seven of them share.
//!
//! **Secrets never reach a line.** A connection string is the single most
//! common thing an agent types at a database, and it carries a password in the
//! middle of it — so redaction happens on the way in, once, for every kind of
//! step and for the expanded command as much as for the summary. There is no
//! path from a tool's title to the screen that does not pass through
//! [`redact`], which is the only arrangement that can be checked.

use crate::acp::ToolKind;
use crate::chat::activity::{self, ActivityKind};
use crate::chat::model::ChatItem;

/// What stands in for a secret once it has been taken out.
///
/// Round dots rather than asterisks: an asterisk is a shell glob and a wildcard
/// in half the query languages an agent types, so a masked argument set in them
/// reads as a command somebody could have run.
pub const MASK: &str = "••••••";

/// Key names whose value is a secret.
///
/// Matched as a whole key or as the last `_`, `-` or `.` separated part of one,
/// which is what makes `SQL_PASSWORD`, `--db-password` and a connection
/// string's `Pwd` all hit while `bypass` does not. Bare `key` is deliberately
/// absent: `--key=value` is an ordinary flag in more tools than it is a
/// credential in.
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "pass",
    "token",
    "secret",
    "credential",
    "credentials",
    "apikey",
    "api_key",
    "accesskey",
    "access_key",
    "secretkey",
    "secret_key",
    "privatekey",
    "private_key",
    "sas",
];

/// Flags whose *next* argument is a secret, for tools that take one positionally.
const SECRET_FLAGS: &[&str] = &["-p", "-P", "--password", "--token", "--api-key", "--secret"];

/// Whether `key` names a secret.
fn is_secret_key(key: &str) -> bool {
    let key = key.trim().trim_start_matches('-').to_ascii_lowercase();
    SECRET_KEYS.iter().any(|secret| {
        key == *secret
            || key.ends_with(&format!("_{secret}"))
            || key.ends_with(&format!("-{secret}"))
            || key.ends_with(&format!(".{secret}"))
    })
}

/// Replace every secret in `text` with [`MASK`].
///
/// **Three shapes, because credentials arrive in three.** A `key=value` pair,
/// which covers connection strings, environment assignments and URL query
/// parameters — and the URL-encoded case with them, since what is masked is the
/// whole value whatever it is spelled in. A `scheme://user:pass@host` authority,
/// where the password has no name at all. And a flag whose next argument is the
/// secret, which is how every database client on a command line takes one.
///
/// **The user and the host survive**, deliberately: a line reading
/// `sa@10.0.0.5` still says who was connecting to what, which is the whole
/// reason the step is on screen. What is taken out is the part that would let
/// somebody else do it.
pub fn redact(text: &str) -> String {
    let masked = redact_pairs(text);
    let masked = redact_authority(&masked);
    redact_flags(&masked)
}

/// `key=value`, up to whatever ends the value.
fn redact_pairs(text: &str) -> String {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != '=' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        // The key is whatever ran up to this `=` without a separator in it.
        let key_start = out
            .rfind(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '?' | '"' | '\'' | '('))
            .map_or(0, |n| n + 1);
        let key: String = out[key_start..].to_string();
        out.push('=');
        i += 1;
        if !is_secret_key(&key) {
            continue;
        }
        // A quoted value runs to its closing quote; an unquoted one to the
        // first thing that could not be part of it.
        let quote = bytes.get(i).copied().filter(|c| matches!(c, '"' | '\''));
        if let Some(quote) = quote {
            out.push(quote);
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            out.push_str(MASK);
            if i < bytes.len() {
                out.push(quote);
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_whitespace()
            && !matches!(bytes[i], ';' | '&' | '"' | '\'' | ')' | ',')
        {
            i += 1;
        }
        if i > start {
            out.push_str(MASK);
        }
    }
    out
}

/// `scheme://user:pass@host`.
fn redact_authority(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        // The authority ends at the first thing that cannot be in one.
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | ';'))
            .unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        match authority.find('@').and_then(|at| {
            let user = &authority[..at];
            user.find(':').map(|colon| (colon, at))
        }) {
            Some((colon, at)) => {
                out.push_str(&authority[..=colon]);
                out.push_str(MASK);
                out.push_str(&authority[at..]);
            }
            None => out.push_str(authority),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `-p secret`, `--password secret`.
///
/// **Only the argument is rewritten; every byte around it survives.** This runs
/// over the command a permission card shows and the well an opened step quotes,
/// both of which claim to be what the agent actually ran — so splitting on
/// whitespace and joining back with single spaces, which is how this was
/// written, silently reflowed a heredoc and squashed the indentation out of a
/// script. What is masked is a span; the gaps between spans are copied.
fn redact_flags(text: &str) -> String {
    let spans = token_spans(text);
    let mut masked: Vec<(usize, usize, String)> = Vec::new();
    let mut mask_next: Option<()> = None;
    for &(start, end) in &spans {
        let token = &text[start..end];
        if mask_next.take().is_some() && !token.starts_with('-') {
            // A flag followed by another flag took no argument after all.
            masked.push((start, end, MASK.to_string()));
            continue;
        }
        // The glued form every one of these also accepts.
        let glued = SECRET_FLAGS.iter().find(|flag| {
            token.len() > flag.len()
                && token.starts_with(**flag)
                && !token[flag.len()..].starts_with('-')
        });
        if let Some(flag) = glued {
            masked.push((start + flag.len(), end, MASK.to_string()));
            continue;
        }
        if SECRET_FLAGS.contains(&token) {
            mask_next = Some(());
        }
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, with) in masked {
        out.push_str(&text[cursor..start]);
        out.push_str(&with);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// Where each whitespace-separated token of `text` begins and ends.
fn token_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(from)) => {
                spans.push((from, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        spans.push((from, text.len()));
    }
    spans
}

/// One step of a run, as the line a child row draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepLine {
    /// `target · action`, already redacted and already stripped of whatever it
    /// shares with its neighbours.
    pub summary: String,
    /// This step is the one before it, run again after it failed.
    pub retry: bool,
}

/// Flags whose next argument names what a command is pointed *at*.
const TARGET_FLAGS: &[&str] = &[
    "-S",
    "-h",
    "-H",
    "--host",
    "--server",
    "--hostname",
    "-d",
    "--database",
    "-f",
    "--file",
    "-C",
    "--directory",
    "--url",
];

/// Flags whose next argument is what a command *does* there.
const ACTION_FLAGS: &[&str] = &[
    "-Q",
    "-q",
    "-c",
    "-e",
    "-i",
    "--query",
    "--command",
    "--eval",
    "--execute",
    "--input-file",
];

/// The two things that tell one step from the next: what it was pointed at, and
/// what it did there.
///
/// **A heuristic, and deliberately a shallow one.** Every client names its host
/// and its query differently and there is no parsing a shell for real, so this
/// reads the flags the common ones use, then falls back on shape: something
/// holding an `@` or looking like an address is a target, something holding a
/// path separator is a file, and the first word that is neither is what the
/// command was asked to do. Where it finds nothing it says so by returning
/// nothing, and the caller prints the shortened command instead — which is
/// worse to read and never wrong.
pub fn target_and_action(command: &str) -> (Option<String>, Option<String>) {
    let tokens = shell_tokens(command);
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let mut target = None;
    let mut action = None;
    let mut i = 0;
    while i < refs.len() {
        let token = refs[i];
        if target.is_none() && TARGET_FLAGS.contains(&token) {
            target = refs.get(i + 1).map(|value| value.to_string());
            i += 2;
            continue;
        }
        if action.is_none() && ACTION_FLAGS.contains(&token) {
            action = refs.get(i + 1).map(|value| value.to_string());
            i += 2;
            continue;
        }
        i += 1;
    }
    // Shape, where no flag said so. A flag's own argument is never re-read as
    // one of these, which is why this runs as a second pass.
    let flagged: Vec<&str> = refs
        .iter()
        .copied()
        .filter(|token| !token.starts_with('-'))
        .collect();
    if target.is_none() {
        target = flagged
            .iter()
            .skip(1)
            .find(|token| looks_like_host(token))
            .map(|token| token.to_string());
    }
    if target.is_none() {
        target = flagged
            .iter()
            .skip(1)
            .find(|token| token.contains('/') || token.contains('.'))
            .map(|token| activity::short_path(token));
    }
    if action.is_none() {
        // **Every word after the program, not the first one.** Taken as the
        // first alone, `docker compose up` and `docker compose down` both came
        // out as `compose` -- two rows reading the same because the word that
        // differed was one past where this stopped looking.
        let words: Vec<&str> = flagged
            .iter()
            .skip(1)
            .copied()
            .filter(|token| Some(token.to_string()) != target && !looks_like_host(token))
            .collect();
        action = (!words.is_empty()).then(|| words.join(" "));
    }
    (
        target.map(|t| activity::first_line_trunc(&t, 60)),
        action.map(|a| activity::first_line_trunc(&a, 90)),
    )
}

fn looks_like_host(token: &str) -> bool {
    if token.contains('@') {
        return true;
    }
    // An address, or a name with a port on it. A bare dotted word is left to
    // the path branch, which is right far more often than not.
    let head = token.split(['/', ',']).next().unwrap_or(token);
    head.contains(':') && !head.contains("://") && head.split(':').count() == 2
        || head
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            && head.split('.').count() == 4
}

/// Split on whitespace, keeping a quoted run together and dropping its quotes.
fn shell_tokens(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in command.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if matches!(c, '"' | '\'') => quote = Some(c),
            None if c.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What one step was pointed at and what it did there, before the run it
/// belongs to has had its say.
///
/// `target` is `None` for anything that is not a command: a file already *is*
/// its own target, and a thought has neither.
fn step_parts(item: &ChatItem) -> (Option<String>, Option<String>) {
    match item {
        ChatItem::Thought(thought) => (None, thought.elapsed_secs.map(|secs| format!("{secs}s"))),
        ChatItem::Permission(p) => (
            None,
            Some(redact(&activity::first_line_trunc(&p.req.title, 240))),
        ),
        ChatItem::Ask(a) => (None, Some(activity::first_line_trunc(&a.req.message, 240))),
        ChatItem::Tool(tool) => {
            let presented = activity::presentation(tool);
            match presented.kind {
                // A command is the case this is all for: it is the one subject
                // that is a whole sentence rather than a name, and seven of
                // them in a row are seven near-identical sentences.
                ActivityKind::Run
                | ActivityKind::Test
                | ActivityKind::Check
                | ActivityKind::Build
                | ActivityKind::Other
                    if tool.call.kind == ToolKind::Execute =>
                {
                    let command = redact(&presented.subject);
                    match target_and_action(&command) {
                        (target, Some(action)) if Some(&action) != target.as_ref() => {
                            (target, Some(action))
                        }
                        // Nothing told the two apart, so the command itself is
                        // the line: worse to read than a pair, and never wrong.
                        _ => (None, Some(command)),
                    }
                }
                _ => (
                    None,
                    Some(redact(&activity::short_path(&presented.subject))),
                ),
            }
        }
        _ => (None, None),
    }
}

/// Whether a step failed, for the rules that are about outcomes.
fn failed(item: &ChatItem) -> bool {
    matches!(item, ChatItem::Tool(tool) if tool.call.status == crate::acp::ToolStatus::Failed)
}

/// Every step of a run, as the lines its child rows draw — and the one thing
/// they all have in common, which belongs to the row above them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLines {
    pub lines: Vec<StepLine>,
    /// What every step in the run was pointed at, where that is one thing.
    ///
    /// **It moves up to the parent.** Seven commands against one database are
    /// seven lines each opening with the same address, which is the column that
    /// tells them apart spent on the part that does not — so the address is
    /// said once, in the row that stands for all seven, and the children are
    /// left with only what differs.
    pub shared: Option<String>,
}

/// Read a run's steps as a set.
///
/// **As a set, not one at a time.** What a reader needs from seven lines is the
/// part that differs — so a target they all share is lifted out, a prefix they
/// all share is taken off, and a step that is the one before it run again after
/// a failure is marked rather than left looking like a duplicate. All three
/// rules need the neighbours, which is why this takes the run rather than the
/// step.
pub fn step_lines(members: &[&ChatItem]) -> RunLines {
    let parts: Vec<(Option<String>, Option<String>)> =
        members.iter().map(|item| step_parts(item)).collect();

    // One target for the whole run, or none: two different addresses have to
    // stay on the lines that name them.
    let targets: Vec<&String> = parts.iter().filter_map(|(t, _)| t.as_ref()).collect();
    let shared = (targets.len() == parts.len() && targets.len() > 1)
        .then(|| targets.first().copied())
        .flatten()
        .filter(|first| targets.iter().all(|t| t == first))
        .cloned();

    let mut summaries: Vec<String> = parts
        .iter()
        .map(|(target, action)| {
            let action = action.clone().unwrap_or_default();
            match target {
                // Said once above, so not said again here.
                Some(_) if shared.is_some() => action,
                Some(target) if !action.is_empty() => format!("{target} · {action}"),
                Some(target) => target.clone(),
                None => action,
            }
        })
        .collect();

    // **The shared opening comes off only where nothing else told the lines
    // apart.** Two queries against one server already differ by their query, so
    // cutting the words they happen to open with leaves `1` beside
    // `name FROM sys.databases` -- the prefix rule doing damage in exactly the
    // case the target-and-action rule had already handled. Where every line is
    // a bare command it is the only rule there is, and `docker compose up`
    // beside `docker compose down` needs it.
    if parts.iter().all(|(target, _)| target.is_none()) {
        strip_shared_prefix(&mut summaries);
    }

    let lines = summaries
        .into_iter()
        .enumerate()
        .map(|(n, summary)| {
            // A retry is the step after a failure that was pointed at the same
            // place: the words are near enough identical that without saying so
            // the list reads as one line printed twice.
            let retry = n > 0
                && members.get(n - 1).is_some_and(|prev| failed(prev))
                && members
                    .get(n)
                    .zip(members.get(n - 1))
                    .is_some_and(|(now, prev)| same_target(now, prev));
            StepLine { summary, retry }
        })
        .collect();
    RunLines { lines, shared }
}

fn same_target(a: &ChatItem, b: &ChatItem) -> bool {
    let subject = |item: &ChatItem| match item {
        ChatItem::Tool(tool) => Some(activity::presentation(tool).subject),
        _ => None,
    };
    match (subject(a), subject(b)) {
        (Some(a), Some(b)) => {
            let (a, b) = (redact(&a), redact(&b));
            a == b || target_and_action(&a).0 == target_and_action(&b).0
        }
        _ => false,
    }
}

/// Take off whatever every line begins with.
///
/// **Whole words, and never all of a line.** Cutting at a character boundary
/// leaves a line starting mid-token, which is unreadable in a way the shared
/// prefix never was; and a run whose lines are all identical would otherwise be
/// stripped down to nothing at all, which is the one case where the shared part
/// is the only thing there is to say.
fn strip_shared_prefix(lines: &mut [String]) {
    if lines.len() < 2 {
        return;
    }
    let words: Vec<Vec<String>> = lines
        .iter()
        .map(|line| line.split_whitespace().map(str::to_string).collect())
        .collect();
    let shortest = words.iter().map(Vec::len).min().unwrap_or(0);
    let mut shared = 0;
    while shared < shortest.saturating_sub(1)
        && words
            .iter()
            .all(|line| line.get(shared) == words[0].get(shared))
    {
        shared += 1;
    }
    if shared == 0 {
        return;
    }
    for (line, words) in lines.iter_mut().zip(words.iter()) {
        *line = words[shared..].join(" ");
    }
}

/// One kind of work in a cluster, as it reads in the line standing for it.
///
/// **The verb is kept apart from the count.** The line is muted end to end so
/// it stays behind the agent's own words, and the only thing lifting out of it
/// is the verbs — which the renderer can only do if it is told where they are.
/// Joined into one string here, the whole sentence would have to be re-parsed
/// at the call site to find them again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryPart {
    pub verb: String,
    /// What follows it, leading space included: ` 3 files`.
    pub rest: String,
}

/// The one sentence a cluster of activity says about itself.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ClusterSummary {
    /// What is happening right now, where anything is.
    ///
    /// Present tense and first, because a line that opens with what is finished
    /// buries the one part of it that is still changing.
    pub running: Option<SummaryPart>,
    /// What has been done, in the order the kinds of work first appeared.
    pub done: Vec<SummaryPart>,
    pub errors: usize,
    /// Lines added and removed across every edit in the cluster.
    pub added: usize,
    pub removed: usize,
    /// How long the whole stretch of work took, where anything reported it.
    ///
    /// **A sum of what settled, not a clock.** Every step stamps its own
    /// duration once, when it settles, so this is arithmetic over facts that
    /// have stopped changing — where a live total would be a number the line
    /// re-reads on every frame and never comes to rest on.
    pub seconds: u64,
}

impl ClusterSummary {
    /// The whole sentence as one string, for the places that cannot draw runs
    /// of it separately — a test, a notification, an export.
    pub fn plain(&self) -> String {
        let mut out = String::new();
        if let Some(running) = &self.running {
            out.push_str(&running.verb);
            out.push_str(&running.rest);
        }
        for (n, part) in self.done.iter().enumerate() {
            out.push_str(match (n, self.running.is_some()) {
                (0, false) => "",
                (0, true) => " · ",
                _ => ", ",
            });
            out.push_str(&part.verb);
            out.push_str(&part.rest);
        }
        out
    }
}

/// The buckets a cluster's sentence counts in, in the order it prints them.
///
/// **Not [`ActivityKind`] one for one.** What the sentence is for is telling a
/// reader what sort of work happened without their having to open anything, and
/// at that distance a test run and a check run are both "ran something" while a
/// read and a search are not. The finer classification is still what each *row*
/// says; this is the coarser one that fits in a phrase.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Read,
    Search,
    Fetch,
    Edit,
    Command,
    Reason,
    Asked,
    Other,
}

impl Bucket {
    /// The verb and the noun this bucket prints, singular and plural.
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Read => ("read", "file", "files"),
            Self::Search => ("searched", "time", "times"),
            Self::Fetch => ("fetched", "page", "pages"),
            Self::Edit => ("edited", "file", "files"),
            Self::Command => ("ran", "command", "commands"),
            Self::Reason => ("thought", "time", "times"),
            Self::Asked => ("asked", "question", "questions"),
            // **No verb, and always last.** Looking a tool up and stopping a
            // task are things the agent did to itself; given a verb of their
            // own they read as work on the project, and mixed in among the
            // rest they push the steps somebody is actually looking for down
            // the sentence.
            Self::Other => ("", "other step", "other steps"),
        }
    }

    fn of(item: &ChatItem) -> Option<Self> {
        match item {
            ChatItem::Thought(_) => Some(Self::Reason),
            ChatItem::Ask(a) if a.resolved.is_some() => Some(Self::Asked),
            // A grant is not work. It is the user answering, and the row it
            // leaves is already inside the cluster for anybody who opens it.
            ChatItem::Permission(_) => None,
            ChatItem::Tool(tool) => Some(match activity::presentation(tool).kind {
                ActivityKind::Inspect => Self::Read,
                ActivityKind::Search => Self::Search,
                ActivityKind::Fetch => Self::Fetch,
                ActivityKind::Change => Self::Edit,
                ActivityKind::Run
                | ActivityKind::Test
                | ActivityKind::Check
                | ActivityKind::Build => Self::Command,
                ActivityKind::Reason => Self::Reason,
                ActivityKind::Other => Self::Other,
            }),
            _ => None,
        }
    }
}

/// The present-tense opening for a step still in flight.
fn running_part(item: &ChatItem) -> SummaryPart {
    let verb = match Bucket::of(item) {
        Some(Bucket::Read) => "Reading",
        Some(Bucket::Search) => "Searching",
        Some(Bucket::Fetch) => "Fetching",
        Some(Bucket::Edit) => "Editing",
        Some(Bucket::Command) => "Running",
        Some(Bucket::Reason) => "Thinking",
        _ => "Working",
    };
    // **The whole short label, not the cut-down one a child row draws.** A row
    // inside a cluster has its siblings to be told apart from, so its line
    // drops the program they all share; this line has no siblings and no
    // context, so dropping it leaves `Running test` where `Running dotnet
    // test` was wanted.
    let subject = match item {
        ChatItem::Tool(tool) => redact(&activity::presentation(tool).subject),
        _ => String::new(),
    };
    let subject = match item {
        ChatItem::Tool(tool) if tool.call.kind != ToolKind::Execute => {
            activity::short_path(&subject)
        }
        _ => subject,
    };
    SummaryPart {
        verb: verb.to_string(),
        // Cut hard: this sits at the head of a line that has to stay one line,
        // and what follows it is the count of everything already done.
        rest: match subject.trim().is_empty() {
            true => String::new(),
            false => format!(" {}", activity::first_line_trunc(subject.trim(), 40)),
        },
    }
}

/// What a whole cluster of activity says in one muted line.
///
/// **Kinds of work, in the order they first happened, each with a count.** Not
/// one phrase per step, which is the folded form costing as much to scan as the
/// unfolded one; and not a bare total, which says how much happened without
/// saying what. A reader skimming a conversation is asking one question — what
/// did it do, and did anything break — and both answers are here.
pub fn cluster_summary(members: &[&ChatItem]) -> ClusterSummary {
    use crate::acp::ToolStatus;

    let mut order: Vec<Bucket> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut errors = 0;
    let mut added = 0;
    let mut removed = 0;
    let mut seconds = 0;
    let mut running = None;

    for item in members {
        if let ChatItem::Tool(tool) = item {
            match tool.call.status {
                ToolStatus::Failed => errors += 1,
                // The last one still going is what the line leads with: an
                // agent that has moved on from the first is not still on it.
                ToolStatus::Pending | ToolStatus::InProgress => running = Some(*item),
                ToolStatus::Completed => {}
            }
            for (_, plus, minus) in &tool.diff_summary {
                added += plus;
                removed += minus;
            }
            seconds += tool.elapsed_secs.unwrap_or(0);
            // A step in flight has not been done yet, so it is not counted
            // among the things that have.
            if matches!(
                tool.call.status,
                ToolStatus::Pending | ToolStatus::InProgress
            ) {
                continue;
            }
        }
        if matches!(item, ChatItem::Thought(t) if t.elapsed_secs.is_none()) {
            running = Some(*item);
            continue;
        }
        let Some(bucket) = Bucket::of(item) else {
            continue;
        };
        match order.iter().position(|seen| *seen == bucket) {
            Some(at) => counts[at] += 1,
            None => {
                order.push(bucket);
                counts.push(1);
            }
        }
    }

    // Internal steps go to the end wherever they happened.
    if let Some(at) = order.iter().position(|b| *b == Bucket::Other) {
        let bucket = order.remove(at);
        let count = counts.remove(at);
        order.push(bucket);
        counts.push(count);
    }

    let mut done: Vec<SummaryPart> = order
        .into_iter()
        .zip(counts)
        .map(|(bucket, count)| {
            let (verb, singular, plural) = bucket.words();
            let noun = if count == 1 { singular } else { plural };
            match verb.is_empty() {
                true => SummaryPart {
                    verb: String::new(),
                    rest: format!("{count} {noun}"),
                },
                false => SummaryPart {
                    verb: verb.to_string(),
                    rest: format!(" {count} {noun}"),
                },
            }
        })
        .collect();

    // **Only the first word of the sentence is capitalised**, and only where
    // the sentence starts with it: a running cluster opens with its own verb,
    // and a second capital mid-line reads as two sentences run together.
    let running = running.map(running_part);
    if let (None, Some(first)) = (&running, done.first_mut()) {
        let head = match first.verb.is_empty() {
            true => &mut first.rest,
            false => &mut first.verb,
        };
        let mut chars = head.chars();
        if let Some(c) = chars.next() {
            *head = c.to_uppercase().collect::<String>() + chars.as_str();
        }
    }

    ClusterSummary {
        running,
        done,
        errors,
        added,
        removed,
        seconds,
    }
}

/// How a run of steps ended, which is not the same question as whether
/// anything in it went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing failed.
    Clean,
    /// Something failed and the run went on to succeed anyway.
    Recovered,
    /// The run ended on a failure.
    Failed,
    /// Still going.
    Running,
}

/// Where a run stands, and how many of its steps failed getting there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunOutcome {
    pub outcome: Outcome,
    pub errors: usize,
}

/// Read a run's ending rather than its worst moment.
///
/// **A failure that was fixed is not a failure.** Marking the parent row red
/// because one of seven commands exited non-zero is an alarm for something that
/// is already dealt with — and a reader who has learned that the red mark means
/// nothing has learned to ignore the one that means something. So the mark
/// follows the *last* step: red only if the run ended badly, and a warning
/// where it ended well after stumbling. The count of what went wrong is still
/// said, beside it, in words.
pub fn run_outcome(members: &[&ChatItem]) -> RunOutcome {
    use crate::acp::ToolStatus;
    let mut errors = 0;
    let mut running = false;
    let mut last: Option<bool> = None;
    for item in members {
        match item {
            ChatItem::Tool(tool) => match tool.call.status {
                ToolStatus::Failed => {
                    errors += 1;
                    last = Some(false);
                }
                ToolStatus::Completed => last = Some(true),
                ToolStatus::Pending | ToolStatus::InProgress => running = true,
            },
            ChatItem::Thought(thought) => running |= thought.elapsed_secs.is_none(),
            _ => {}
        }
    }
    let outcome = match (running, last) {
        (true, _) => Outcome::Running,
        (false, Some(false)) => Outcome::Failed,
        (false, _) if errors > 0 => Outcome::Recovered,
        (false, _) => Outcome::Clean,
    };
    RunOutcome { outcome, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{ToolCall, ToolKind, ToolStatus};
    use crate::chat::model::ToolItem;

    fn run(title: &str, status: ToolStatus) -> ChatItem {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind: ToolKind::Execute,
            status,
            content: Vec::new(),
        }))
    }

    #[test]
    fn a_secret_never_reaches_a_line() {
        let connection = "sqlcmd -S 10.0.0.5 -U sa -P Hunter2 \
             -Q \"SELECT 1\"";
        let masked = redact(connection);
        assert!(!masked.contains("Hunter2"), "{masked}");
        // The user and the host are what make the line worth drawing at all.
        assert!(masked.contains("sa"), "{masked}");
        assert!(masked.contains("10.0.0.5"), "{masked}");

        let pairs = "Server=db;User Id=sa;Password=p@ss w0rd;Encrypt=false";
        let masked = redact(pairs);
        assert!(!masked.contains("p@ss"), "{masked}");
        assert!(masked.contains("Encrypt=false"), "{masked}");

        // URL-encoded is the same value in another spelling, so masking the
        // whole of it covers both without knowing which it was.
        let url = "curl 'https://api.example.com/v1?api_key=abc%2Fdef&page=2'";
        let masked = redact(url);
        assert!(!masked.contains("abc%2Fdef"), "{masked}");
        assert!(masked.contains("page=2"), "{masked}");

        let authority = "psql postgres://sa:s3cret@10.0.0.5:5432/app";
        let masked = redact(authority);
        assert!(!masked.contains("s3cret"), "{masked}");
        assert!(masked.contains("sa:"), "{masked}");
        assert!(masked.contains("10.0.0.5"), "{masked}");

        // A word that merely ends in the letters of a secret is not one.
        assert_eq!(redact("--bypass=true"), "--bypass=true");
        assert_eq!(redact("--key=value"), "--key=value");

        // **Everything around the secret survives byte for byte.** This runs
        // over the command a permission card shows, which claims to be what the
        // agent actually ran -- so a script that came back reflowed would be a
        // grant given on one text and spent on another.
        let script =
            "#!/usr/bin/env bash\nset -euo pipefail\n\tmysql -p hunter2 <<'EOF'\n  SELECT 1;\nEOF";
        let masked = redact(script);
        assert!(!masked.contains("hunter2"), "{masked}");
        assert!(
            masked.starts_with("#!/usr/bin/env bash\nset -euo pipefail\n\tmysql"),
            "{masked}"
        );
        assert!(masked.contains("<<'EOF'\n  SELECT 1;\nEOF"), "{masked}");
        // A command with no secret in it comes back exactly as it went in.
        let plain = "git commit -m 'two  spaces'\n\tdocs/plan.md";
        assert_eq!(redact(plain), plain);
    }

    #[test]
    fn two_steps_of_a_run_do_not_read_the_same() {
        let members = [
            run(
                "sqlcmd -S 10.0.0.5 -U sa -P x -Q \"SELECT 1\"",
                ToolStatus::Completed,
            ),
            run(
                "sqlcmd -S 10.0.0.5 -U sa -P x -Q \"SELECT name FROM sys.databases\"",
                ToolStatus::Completed,
            ),
        ];
        let refs: Vec<&ChatItem> = members.iter().collect();
        let run = step_lines(&refs);
        assert_ne!(run.lines[0].summary, run.lines[1].summary, "{run:?}");
        assert!(run.lines[0].summary.contains("SELECT 1"), "{run:?}");
        // The one thing both were pointed at moves up to the parent, so the
        // children are left with only what differs.
        assert_eq!(run.shared.as_deref(), Some("10.0.0.5"), "{run:?}");
        assert!(!run.lines[0].summary.contains("10.0.0.5"), "{run:?}");
        // The secret is gone from the line as well as from the command.
        assert!(!run.lines[1].summary.contains(" x"), "{run:?}");
    }

    #[test]
    fn a_step_run_again_after_a_failure_says_so() {
        let members = [
            run("sqlcmd -S 10.0.0.5 -Q \"SELECT 1\"", ToolStatus::Failed),
            run("sqlcmd -S 10.0.0.5 -Q \"SELECT 1\"", ToolStatus::Completed),
        ];
        let refs: Vec<&ChatItem> = members.iter().collect();
        let run = step_lines(&refs);
        assert!(!run.lines[0].retry);
        assert!(run.lines[1].retry, "{run:?}");
    }

    fn read(title: &str) -> ChatItem {
        ChatItem::Tool(ToolItem::new(ToolCall {
            id: title.into(),
            title: title.into(),
            description: None,
            kind: ToolKind::Read,
            status: ToolStatus::Completed,
            content: Vec::new(),
        }))
    }

    /// **The one line a cluster says about itself.**
    ///
    /// Kinds of work in the order they happened, each with a count — not one
    /// phrase per step, which costs as much to scan folded as unfolded, and not
    /// a bare total, which says how much without saying what.
    #[test]
    fn a_cluster_says_what_it_did_and_in_what_order() {
        let members = [
            read("a.rs"),
            read("b.rs"),
            run("cargo build", ToolStatus::Completed),
            read("c.rs"),
            run("cargo test", ToolStatus::Failed),
        ];
        let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
        // Three reads are one phrase, wherever the third one happened; the
        // order is where each *kind* first appeared.
        assert_eq!(summary.plain(), "Read 3 files, ran 2 commands");
        assert_eq!(summary.errors, 1);
        assert!(summary.running.is_none());
        // Only the opening word is capitalised: a second capital mid-line
        // reads as two sentences run together.
        assert_eq!(summary.done[1].verb, "ran");
    }

    /// **A duration is stamped once, when a step settles.**
    ///
    /// The other two readings are both wrong: measured from the row that draws
    /// it, the number changes every frame and never comes to rest even after
    /// the step has; measured again on each later update — and several arrive
    /// after the status does, since content follows it — the clock restarts and
    /// the step reports the time since it finished.
    #[test]
    fn a_step_is_timed_once_and_the_cluster_adds_them_up() {
        use crate::acp::{AcpEvent, ToolCallUpdate};

        let mut chat = crate::chat::model::Chat::default();
        chat.apply(AcpEvent::ToolCall(ToolCall {
            id: "one".into(),
            title: "cargo build".into(),
            description: None,
            kind: ToolKind::Execute,
            status: ToolStatus::InProgress,
            content: Vec::new(),
        }));
        let settle = |chat: &mut crate::chat::model::Chat| {
            chat.apply(AcpEvent::ToolUpdate(ToolCallUpdate {
                id: "one".into(),
                status: Some(ToolStatus::Completed),
                title: None,
                description: None,
                content: None,
            }));
        };
        settle(&mut chat);
        let stamped = match &chat.items[0] {
            ChatItem::Tool(t) => t.elapsed_secs,
            _ => unreachable!(),
        };
        assert!(stamped.is_some(), "settling is when the duration exists");

        // Content arrives after the status on a real adapter, and each of those
        // is another update to the same card.
        settle(&mut chat);
        let again = match &chat.items[0] {
            ChatItem::Tool(t) => t.elapsed_secs,
            _ => unreachable!(),
        };
        assert_eq!(stamped, again, "a later update must not restart the clock");

        // A step that arrived already settled was timed by whoever ran it.
        let done = run("cargo test", ToolStatus::Completed);
        assert!(matches!(&done, ChatItem::Tool(t) if t.elapsed_secs.is_none()));
    }

    /// **The exit status is kept on the way past, or it is lost.**
    ///
    /// A finished terminal is folded into the card it belonged to at turn end,
    /// and after that the code is a line inside a string. Recovering it would
    /// mean parsing the footer back out — a number recovered by parsing is a
    /// number that is wrong the first time the wording changes — so it is
    /// lifted onto the step in the one moment both are in hand.
    #[test]
    fn an_exit_status_survives_the_terminal_it_came_from() {
        use crate::acp::AcpEvent;

        let mut chat = crate::chat::model::Chat::default();
        chat.apply(AcpEvent::ToolCall(ToolCall {
            id: "t".into(),
            title: "cargo test".into(),
            description: None,
            kind: ToolKind::Execute,
            status: ToolStatus::InProgress,
            content: vec![crate::acp::ToolContent::Terminal("term-1".into())],
        }));
        chat.apply(AcpEvent::TerminalExit {
            terminal_id: "term-1".into(),
            exit_code: Some(101),
        });
        chat.apply(AcpEvent::TurnEnded {
            stop_reason: "end_turn".into(),
        });

        let ChatItem::Tool(step) = &chat.items[0] else {
            unreachable!()
        };
        assert_eq!(step.exit_code, Some(101));
        // And the terminal is gone: the map only ever holds live ones.
        assert!(chat.terminals.is_empty());
    }

    /// A cluster of one is a cluster, and says so the same way.
    #[test]
    fn one_step_still_gets_a_sentence() {
        let summary = cluster_summary(&[&read("src/lib.rs")]);
        assert_eq!(summary.plain(), "Read 1 file");
    }

    /// **What is still happening leads, in the present tense.** A line opening
    /// with what is finished buries the one part of it still changing.
    #[test]
    fn a_running_cluster_leads_with_what_it_is_doing() {
        let members = [
            read("a.rs"),
            read("b.rs"),
            run("dotnet test ./src/Api.Tests", ToolStatus::InProgress),
        ];
        let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
        let running = summary.running.as_ref().expect("something is running");
        assert_eq!(running.verb, "Running");
        assert!(running.rest.contains("dotnet"), "{running:?}");
        // The step in flight has not been done yet, so it is not counted among
        // the things that have.
        assert_eq!(summary.plain(), "Running dotnet test · read 2 files");
    }

    /// Internal steps go to the end wherever they happened, and take no verb.
    #[test]
    fn the_agents_own_housekeeping_goes_last() {
        let other = |title: &str| {
            ChatItem::Tool(ToolItem::new(ToolCall {
                id: title.into(),
                title: title.into(),
                description: None,
                kind: ToolKind::Other,
                status: ToolStatus::Completed,
                content: Vec::new(),
            }))
        };
        let members = [other("ToolSearch"), read("a.rs"), other("TaskStop")];
        let summary = cluster_summary(&members.iter().collect::<Vec<_>>());
        assert_eq!(summary.plain(), "Read 1 file, 2 other steps");
    }

    #[test]
    fn a_run_is_read_by_how_it_ended() {
        let clean = [run("a", ToolStatus::Completed)];
        assert_eq!(
            run_outcome(&clean.iter().collect::<Vec<_>>()),
            RunOutcome {
                outcome: Outcome::Clean,
                errors: 0
            }
        );

        // Fixed, so not an alarm — but the count of what went wrong is kept.
        let recovered = [
            run("a", ToolStatus::Failed),
            run("b", ToolStatus::Failed),
            run("c", ToolStatus::Completed),
        ];
        assert_eq!(
            run_outcome(&recovered.iter().collect::<Vec<_>>()),
            RunOutcome {
                outcome: Outcome::Recovered,
                errors: 2
            }
        );

        let broken = [
            run("a", ToolStatus::Completed),
            run("b", ToolStatus::Failed),
        ];
        assert_eq!(
            run_outcome(&broken.iter().collect::<Vec<_>>()).outcome,
            Outcome::Failed
        );

        let live = [
            run("a", ToolStatus::Failed),
            run("b", ToolStatus::InProgress),
        ];
        assert_eq!(
            run_outcome(&live.iter().collect::<Vec<_>>()).outcome,
            Outcome::Running
        );
    }

    /// A subcommand is not always the first word after the program.
    #[test]
    fn a_nested_subcommand_reaches_the_word_that_differs() {
        let members = [
            run("docker compose up -d", ToolStatus::Completed),
            run("docker compose down", ToolStatus::Completed),
        ];
        let refs: Vec<&ChatItem> = members.iter().collect();
        let lines = step_lines(&refs).lines;
        assert_ne!(lines[0].summary, lines[1].summary, "{lines:?}");
        assert!(lines[0].summary.contains("up"), "{lines:?}");
        assert!(lines[1].summary.contains("down"), "{lines:?}");
    }

    #[test]
    fn a_shared_opening_is_taken_off_every_line_but_never_all_of_one() {
        let mut lines = vec![
            "docker compose up".to_string(),
            "docker compose down".to_string(),
        ];
        strip_shared_prefix(&mut lines);
        assert_eq!(lines, vec!["up", "down"]);

        // Identical lines keep what they have: the shared part is all there is.
        let mut same = vec!["cargo test".to_string(), "cargo test".to_string()];
        strip_shared_prefix(&mut same);
        assert_eq!(same, vec!["test", "test"]);
    }
}
