//! What a run of steps says about itself: the sentence standing for a cluster,
//! how the run came out, and what a whole turn did to the working tree.
//!
//! **Secrets are taken out here, and only here.** A connection string is the
//! single most common thing an agent types at a database, and it carries a
//! password in the middle of it -- so [`redact`] exists once, for every kind of
//! step and for the expanded command as much as for the summary.
//!
//! **It is a convention and not an invariant, and saying so is the point.**
//! Nothing in the type system stops a renderer printing a tool's title
//! straight, and there is no guard counting the call sites: the ones that
//! matter are the handful in the transcript that draw a command or its
//! arguments, and each calls this by hand. A doc here claiming the path is
//! closed would be the one thing worse than an open path -- a reader who stops
//! checking. If that ever needs to be real, the shape is a wrapper type that
//! can only be built by going through [`redact`], not a sentence in a module
//! header.

use crate::acp::ToolKind;
use crate::chat::activity::{self, ActivityKind};
use crate::chat::model::ChatItem;

/// What stands in for a secret once it has been taken out.
///
/// Round dots rather than asterisks: an asterisk is a shell glob and a wildcard
/// in half the query languages an agent types, so a masked argument set in them
/// reads as a command somebody could have run.
pub(crate) const MASK: &str = "••••••";

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
        // **The last `@`, never the first.** A password may contain one -- it
        // is a legal character there and a common one -- so splitting at the
        // first leaves everything after it outside the span that gets masked:
        // `postgres://sa:p@ssw0rd@host` came back having hidden the `p` and
        // printed the rest of the password. The authority's own separator is
        // the final `@`, because everything before it is the userinfo.
        match authority.rfind('@').and_then(|at| {
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
    let mut mask_next = false;
    for &(start, end) in &spans {
        let token = &text[start..end];
        if std::mem::take(&mut mask_next) && !token.starts_with('-') {
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
            mask_next = true;
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
mod tests;
/// What a turn did to one file, end to end.
///
/// **Only three, because the protocol carries only three.** A diff section is
/// a path, the text before and the text after, so a file that appeared has no
/// before and one that went has no after. A *rename* is two paths and the
/// protocol never sends the other one -- an adapter reports it as one file
/// gone and another arrived, and a fourth verdict here would be one this can
/// never actually return.
///
/// **`Deleted` is a guess, and the only one available.** A file emptied and a
/// file removed arrive identically -- a section whose new text is empty -- so
/// this reads the commoner of the two. What it costs is the letter on the row;
/// the counts and the bar are right either way, since a file with nothing left
/// in it has no untouched remainder whichever happened to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileVerdict {
    Added,
    Modified,
    Deleted,
}

/// One file a turn touched, with everything that happened to it in that turn
/// added together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub verdict: FileVerdict,
    /// Lines the file has once the turn is over, which is what the untouched
    /// part of a ratio bar is a part *of*. Zero for a file that went.
    pub total: usize,
}

impl FileChange {
    /// How much of this file the turn touched, which is what orders the list.
    pub fn touched(&self) -> usize {
        self.added + self.removed
    }
}

/// What a whole turn did to the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnChanges {
    /// In the order the turn first touched each, which is the order the
    /// reader watched it happen in.
    pub files: Vec<FileChange>,
    pub added: usize,
    pub removed: usize,
    /// How long the turn took, from the prompt going out to the turn ending.
    /// Absent on a replayed or legacy turn, which carries no timestamps.
    pub seconds: Option<u64>,
}

/// Every file `items` touched, once each.
///
/// **One row per file and not one per edit.** A turn writes a file, runs the
/// tests, fixes it and writes it again, and three rows saying `src/lib.rs` is
/// the agent's route rather than the result -- which is the question this
/// answers: what is different now. The individual writes are still each in the
/// transcript, in the cluster they happened in, where the route is what is
/// being read.
///
/// `items` is the whole turn, the prompt that began it included: the prompt is
/// what carries the two moments the duration is the difference between.
///
/// `None` where nothing was written. A turn that only read files has nothing
/// to summarise, and a line saying so is a line that appears after every
/// question and reports nothing.
pub fn turn_changes(items: &[&ChatItem]) -> Option<TurnChanges> {
    use crate::acp::ToolContent;

    let mut files: Vec<FileChange> = Vec::new();
    for item in items {
        let ChatItem::Tool(tool) = item else {
            continue;
        };
        // **An edit that failed did not happen.** The diff on a failed call is
        // what the agent proposed, not what is on disk, and counting it puts a
        // file in a list headed "what this turn wrote" that the turn did not
        // write. The step is still in the transcript above, in the cluster it
        // failed in, saying so.
        if tool.call.status == crate::acp::ToolStatus::Failed {
            continue;
        }
        for section in &tool.call.content {
            let ToolContent::Diff { path, old, new } = section else {
                continue;
            };
            let (plus, minus) = crate::chat::model::line_change_counts(old.as_deref(), new);
            // **The verdict is the turn's, not this edit's.** A file created
            // and then edited again was still created by this turn, and a file
            // whose last edit emptied it is gone however it started -- so the
            // first edit decides whether it arrived and the last whether it
            // survived.
            let gone = new.is_empty();
            let total = match gone {
                true => 0,
                false => new.lines().count(),
            };
            match files.iter_mut().find(|seen| seen.path == *path) {
                Some(seen) => {
                    seen.added += plus;
                    seen.removed += minus;
                    seen.total = total;
                    seen.verdict = match (seen.verdict, gone) {
                        (_, true) => FileVerdict::Deleted,
                        (FileVerdict::Added, false) => FileVerdict::Added,
                        (_, false) => FileVerdict::Modified,
                    };
                }
                None => files.push(FileChange {
                    path: path.clone(),
                    added: plus,
                    removed: minus,
                    verdict: match (old.is_none(), gone) {
                        (_, true) => FileVerdict::Deleted,
                        (true, false) => FileVerdict::Added,
                        (false, false) => FileVerdict::Modified,
                    },
                    total,
                }),
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    Some(TurnChanges {
        added: files.iter().map(|f| f.added).sum(),
        removed: files.iter().map(|f| f.removed).sum(),
        seconds: items.iter().find_map(|item| match item {
            ChatItem::User(user) => user
                .sent_at
                .zip(user.completed_at)
                .map(|(out, back)| back.saturating_sub(out)),
            _ => None,
        }),
        files,
    })
}

/// The turn's net diff for one file: what it looked like before the turn's
/// first edit against what it looks like after the last.
///
/// **Computed when a row is opened and never before.** A conversation holds
/// every turn it has had, and diffing every file of every one of them on the
/// chance somebody expands one is work paid a thousand times to be used once.
/// What the summary carries is counts, which the model had already worked out.
///
/// **The turn's diff and not the last edit's**, which is the rule the counts
/// follow too: a file written, tested and written again shows what changed
/// about it, not what the final touch-up was.
pub fn turn_file_diff(items: &[&ChatItem], path: &str) -> Vec<crate::diff::Row> {
    use crate::acp::ToolContent;

    let mut before: Option<&str> = None;
    let mut after = "";
    let mut seen = false;
    for item in items {
        let ChatItem::Tool(tool) = item else {
            continue;
        };
        // Same rule as the counts: a failed edit is not part of what the turn
        // did to this file.
        if tool.call.status == crate::acp::ToolStatus::Failed {
            continue;
        }
        for section in &tool.call.content {
            let ToolContent::Diff { path: at, old, new } = section else {
                continue;
            };
            if at != path {
                continue;
            }
            if !seen {
                before = old.as_deref();
                seen = true;
            }
            after = new;
        }
    }
    crate::diff::rows(before.unwrap_or_default(), after)
}
