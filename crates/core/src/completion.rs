//! Pure `@`-file-mention and `/`-command completion logic. The composer popup
//! is driven entirely by these GUI-free functions: detect an active trigger at
//! the caret, filter candidates, and apply a chosen completion back into the
//! text.

/// What the caret is currently completing, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTrigger {
    pub kind: TriggerKind,
    /// Byte index of the trigger char (`@` or `/`) in the source string.
    pub(crate) start: usize,
    /// The query text typed after the trigger (may be empty).
    pub query: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerKind {
    /// `@` — file mention.
    File,
    /// `/` — slash command (only valid at the very start of the input).
    Command,
}

/// Detect a completion trigger ending at byte offset `caret` in `text`.
///
/// Rules:
/// - An `@` opens a file mention; the query runs from just after `@` to the
///   caret and must not contain whitespace.
/// - A leading `/` (the input starts with it) opens a command; the query is the
///   rest, no whitespace.
pub fn detect(text: &str, caret: usize) -> Option<ActiveTrigger> {
    let caret = caret.min(text.len());
    let before = &text[..caret];

    // Slash command: only when the whole input begins with '/'.
    if let Some(query) = before.strip_prefix('/') {
        if !query.contains(char::is_whitespace) {
            return Some(ActiveTrigger {
                kind: TriggerKind::Command,
                start: 0,
                query: query.to_string(),
            });
        }
    }

    // File mention: nearest '@' to the left with no whitespace between it and
    // the caret, and either at the start or preceded by whitespace.
    if let Some(at) = before.rfind('@') {
        let query = &before[at + 1..];
        let preceded_ok = at == 0
            || before[..at]
                .chars()
                .next_back()
                .map(char::is_whitespace)
                .unwrap_or(false);
        if preceded_ok && !query.contains(char::is_whitespace) {
            return Some(ActiveTrigger {
                kind: TriggerKind::File,
                start: at,
                query: query.to_string(),
            });
        }
    }
    None
}

/// Case-insensitive substring filter over candidate labels, preserving order.
pub fn filter<'a>(candidates: &'a [String], query: &str) -> Vec<&'a String> {
    let q = query.to_lowercase();
    candidates
        .iter()
        .filter(|c| q.is_empty() || c.to_lowercase().contains(&q))
        .collect()
}

/// Where `needle` first occurs in `haystack`, ignoring case.
///
/// This is what lets a row say *why* it matched: the characters inside the
/// range are drawn at full strength and everything around them stays quiet, so
/// the answer is carried by the one axis already spent on reading — no second
/// colour, no weight, no rule under the letters.
///
/// **Compared character by character rather than by lowercasing the whole
/// string and searching that.** Lowercasing is not length-preserving in Unicode
/// — a handful of characters grow a byte or lose one — so a byte offset found
/// in the lowercased copy does not necessarily land on a character boundary in
/// the original, and slicing at it is a panic. Rare enough never to be seen in
/// testing and certain to arrive eventually, since the haystack here is a
/// filename somebody else chose.
pub(crate) fn span(haystack: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    fn lower(c: char) -> char {
        c.to_lowercase().next().unwrap_or(c)
    }
    if needle.is_empty() {
        return None;
    }
    for (start, _) in haystack.char_indices() {
        let mut rest = haystack[start..].char_indices();
        let mut end = start;
        let mut whole = true;
        for want in needle.chars().map(lower) {
            match rest.next() {
                Some((at, got)) if lower(got) == want => end = start + at + got.len_utf8(),
                _ => {
                    whole = false;
                    break;
                }
            }
        }
        if whole {
            return Some(start..end);
        }
    }
    None
}

/// Split a root-relative path into the part that is scanned and the part that
/// tells two of the same name apart.
///
/// The filename leads because that is what a query is typed against and what
/// the eye compares down a column; the folder follows because it only matters
/// once two rows carry the same name.
fn split(path: &str) -> (&str, Option<&str>) {
    match path.rsplit_once('/') {
        Some((parent, name)) if !name.is_empty() => (name, Some(parent)),
        _ => (path, None),
    }
}

/// Every directory the file list passes through, with the number of files
/// under each.
///
/// **Derived from the files rather than walked for.** The scan has already
/// been paid for and already visited every one of these; asking the filesystem
/// again would be a second bounded walk that can disagree with the first, and
/// disagreeing is worse than either answer — a folder offered here that the
/// file list has nothing under is a mention that comes back empty.
///
/// The count is everything *beneath* the directory and not the files sitting
/// directly in it, because that is the question being asked: mentioning a
/// folder offers its listing, and a listing of four is a different thing to
/// accept than a listing of four hundred.
pub fn folders(files: &[String]) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for file in files {
        let mut at = 0;
        while let Some(slash) = file[at..].find('/') {
            at += slash;
            *counts.entry(&file[..at]).or_default() += 1;
            at += 1;
        }
    }
    counts
        .into_iter()
        .map(|(dir, under)| (dir.to_string(), under))
        .collect()
}

/// The short label a long description opens with.
///
/// **A command's description is not written for a menu.** It is written for a
/// model deciding whether to invoke the thing, so it lists triggers, phrasings
/// and counter-examples, and it runs to several sentences. Put in a row it
/// fills the column and ends in an ellipsis on every line, which makes the
/// second column a wall of grey with no information in it — worse than blank,
/// because it costs the width as well.
///
/// What saves it is that such a description almost always *opens* with the
/// short form: a title sentence, then the long explanation. So the cut is at
/// the first sentence end, and what is left is usually exactly the label
/// somebody would have written by hand.
///
/// **A comma is not a cut.** It was tried and it is wrong more often than it is
/// right: a first sentence with a comma in it is ordinary prose, and cutting
/// there yields a fragment rather than a title. `;` is, because a semicolon in
/// a first sentence is already joining two statements that could stand apart.
/// A colon is not, because the clause after it is usually the content —
/// `Triggers on: chart, graph` cut at the colon says `Triggers on`.
pub(crate) fn summary(description: &str) -> &str {
    let description = description.trim();
    let end = description
        .match_indices(". ")
        .chain(description.match_indices("; "))
        .map(|(at, _)| at)
        .min()
        // A description that is a single sentence still carries its full stop,
        // and a label does not need one.
        .unwrap_or(
            description
                .len()
                .saturating_sub(if description.ends_with('.') { 1 } else { 0 }),
        );
    description[..end].trim()
}

/// One row of the `/` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// What replaces the trigger and its query: the whole name, prefix
    /// included, because that is what the agent answers to.
    pub insert: String,
    /// The leading column: the name with its namespace taken off, since the
    /// heading above the run already carries it.
    pub name: String,
    /// The run this belongs under, where it is in one.
    pub namespace: Option<String>,
    /// The second column: one short line, or nothing where the agent sent no
    /// description at all.
    pub summary: Option<String>,
    /// Where the query matched the name as drawn.
    pub name_span: Option<std::ops::Range<usize>>,
}

/// The `/` list for one query: the agent's own commands first, then each
/// namespace as a run of its own.
///
/// **A namespaced name is split rather than printed whole.** Printed whole, the
/// prefix is repeated on every row of its run and it is repeated at the *front*
/// — which is where the eye lands and where the part that differs ought to be.
/// Ten rows reading `some-plugin:` followed by the two characters that vary is
/// a column with no column in it. The prefix is a property of the run, so it
/// goes on the heading and comes off the rows.
///
/// The query still matches the **whole** name, prefix included, so typing the
/// namespace still finds its commands — what it does not do is light anything,
/// since the characters it matched are no longer drawn on the row. The heading
/// above them is what says why they are there.
///
/// Unprefixed commands lead, because they are the agent's own and are what a
/// bare `/` is asking about.
pub fn commands(
    all: &[crate::acp::SlashCommand],
    query: &str,
    per_group: usize,
) -> (Vec<Command>, usize) {
    let row = |c: &crate::acp::SlashCommand| {
        let (namespace, name) = match c.name.split_once(':') {
            Some((ns, rest)) if !ns.is_empty() && !rest.is_empty() => {
                (Some(ns.to_string()), rest.to_string())
            }
            _ => (None, c.name.clone()),
        };
        let short = summary(&c.description);
        // The namespace said twice is the same repetition one column over.
        // Stripped here rather than at the renderer, because whether the
        // description opens with it is a fact about the text.
        let short = namespace
            .as_deref()
            .and_then(|ns| {
                let rest = short
                    .get(..ns.len())?
                    .eq_ignore_ascii_case(ns)
                    .then(|| short[ns.len()..].trim_start_matches([':', '-', ' ', '—']))?;
                (!rest.is_empty()).then_some(rest)
            })
            .unwrap_or(short);
        Command {
            insert: c.name.clone(),
            name_span: span(&name, query),
            name,
            namespace,
            summary: (!short.is_empty()).then(|| short.to_string()),
        }
    };

    let matched: Vec<Command> = all
        .iter()
        .filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query.to_lowercase()))
        .map(row)
        .collect();

    // Stable within a run and the runs in a fixed order, so walking the list
    // twice with the same query walks the same list. `None` sorting before
    // `Some` is what puts the agent's own commands first, and it is load-bearing
    // rather than incidental: a bare `/` is asking what this agent does, and a
    // list that opened on somebody's plugin would be answering a different
    // question.
    let mut namespaces: Vec<Option<String>> = matched.iter().map(|c| c.namespace.clone()).collect();
    namespaces.sort();
    namespaces.dedup();

    let mut rows = Vec::new();
    let mut held = 0usize;
    for ns in namespaces {
        let run: Vec<Command> = matched
            .iter()
            .filter(|c| c.namespace == ns)
            .cloned()
            .collect();
        held += run.len().saturating_sub(per_group);
        rows.extend(run.into_iter().take(per_group));
    }
    (rows, held)
}

/// Which of the three things an `@` row offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MentionKind {
    /// A file in the project.
    File,
    /// A directory. Accepting one offers its *listing*.
    Folder,
    /// A path this session has already touched — edited, read, or attached.
    Artifact,
}

/// One row of the `@` list, already in the columns it is drawn in.
///
/// Split here rather than at the renderer because every part of it is a rule
/// about the text: which half of a path leads, where a query matched, how a
/// folder's weight is said. A renderer that re-derived any of them would be a
/// second answer to a question that has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub kind: MentionKind,
    /// What the trigger and its query are replaced with.
    pub insert: String,
    /// The leading column: what is scanned and compared.
    pub name: String,
    /// The trailing column: where that name is.
    pub parent: Option<String>,
    /// A folder's weight, in words.
    pub note: Option<String>,
    /// Where the query matched the name, if it did.
    pub name_span: Option<std::ops::Range<usize>>,
    /// Where it matched the parent, where it did not match the name. Only one
    /// of the two is ever set: the query is one run of characters, and lighting
    /// it in both columns says it was found twice.
    pub parent_span: Option<std::ops::Range<usize>>,
}

impl Mention {
    fn new(kind: MentionKind, path: &str, insert: String, note: Option<String>) -> Self {
        let (name, parent) = split(path);
        Self {
            kind,
            insert,
            name: name.to_string(),
            parent: parent.map(str::to_string),
            note,
            name_span: None,
            parent_span: None,
        }
    }

    /// Light the run the query matched, preferring the name.
    ///
    /// The name first because that is where a query is aimed: a search for
    /// `main` against `src/main.rs` means the file, and lighting the `main` in
    /// a folder called `main-thread` instead would answer a question nobody
    /// asked. The parent is the fallback for the query that only occurs there,
    /// which is what a reader needs when the row's name has nothing lit in it
    /// and they are trying to work out why it is in the list at all.
    fn lit(mut self, query: &str) -> Self {
        match span(&self.name, query) {
            Some(at) => self.name_span = Some(at),
            None => {
                self.parent_span = self
                    .parent
                    .as_deref()
                    .and_then(|parent| span(parent, query));
            }
        }
        self
    }
}

/// The whole `@` list for one query: files, then folders, then what this
/// session has already touched.
///
/// **Each group is capped on its own**, not out of one shared budget. Sharing
/// one would let the first group spend it: a bare `@` in a large repository
/// matches every file there is, and artifacts — the group most likely to hold
/// what the user is reaching for, since it is the thing that just happened —
/// would be the group that never appeared. The count returned is everything
/// held back across all three, because from the reader's side there is one
/// list and one question about it.
///
/// A group that matched nothing contributes no rows, and therefore no heading:
/// the heading belongs to the row under it, so an empty group cannot leave one
/// behind.
/// Rows the artifact run shows before the rest are counted rather than drawn.
///
/// **Small on purpose, and counted rather than cut off upstream.** The claim
/// this group makes is "the thing that just happened", and it is only true of
/// the last few — a long recency list is the project's own file list again, in
/// a worse order and under a heading promising something it no longer delivers.
/// But the cut has to happen *after* the query has been applied and where the
/// count of what was held back can be reported, or a query aimed at something
/// older finds nothing here and nothing says why.
const ARTIFACT_ROWS: usize = 8;

pub fn mentions(
    files: &[String],
    folders: &[(String, usize)],
    artifacts: &[String],
    query: &str,
    per_group: usize,
) -> (Vec<Mention>, usize) {
    let mut rows = Vec::new();
    let mut held = 0usize;

    let mut take = |matched: Vec<Mention>, cap: usize| {
        held += matched.len().saturating_sub(cap);
        rows.extend(matched.into_iter().take(cap));
    };

    take(
        filter(files, query)
            .into_iter()
            .map(|path| Mention::new(MentionKind::File, path, path.clone(), None).lit(query))
            .collect(),
        per_group,
    );
    // Filtered where the counts already are. Copying the names out to reuse
    // `filter` meant cloning every one of them and then walking back into the
    // slice per match to recover the count it had been separated from -- two
    // costs for the convenience of one call, on the one group whose rows carry
    // a second fact about themselves.
    let q = query.to_lowercase();
    take(
        folders
            .iter()
            .filter(|(dir, _)| q.is_empty() || dir.to_lowercase().contains(&q))
            .map(|(dir, n)| {
                Mention::new(
                    MentionKind::Folder,
                    dir,
                    // The trailing slash is what makes this a listing rather
                    // than a read: it is the shape an agent already reads as a
                    // directory, so nothing here has to teach it a second one.
                    format!("{dir}/"),
                    Some(match n {
                        1 => "1 file".to_string(),
                        n => format!("{n} files"),
                    }),
                )
                .lit(query)
            })
            .collect(),
        per_group,
    );
    take(
        filter(artifacts, query)
            .into_iter()
            .map(|path| Mention::new(MentionKind::Artifact, path, path.clone(), None).lit(query))
            .collect(),
        per_group.min(ARTIFACT_ROWS),
    );

    (rows, held)
}

/// Apply `choice` for `trigger` to `text`, replacing the trigger+query span.
/// Returns the new text and the new caret byte offset.
///
/// A file mention keeps its `@` prefix and gets a trailing space; a command
/// replaces the whole `/query` with `/choice` and a trailing space.
pub fn apply(text: &str, caret: usize, trigger: &ActiveTrigger, choice: &str) -> (String, usize) {
    let caret = caret.min(text.len());
    let replacement = match trigger.kind {
        TriggerKind::File => format!("@{choice} "),
        TriggerKind::Command => format!("/{choice} "),
    };
    let mut out = String::with_capacity(text.len() + replacement.len());
    out.push_str(&text[..trigger.start]);
    out.push_str(&replacement);
    out.push_str(&text[caret..]);
    let new_caret = trigger.start + replacement.len();
    (out, new_caret)
}

/// Directories never descended into when scanning for `@`-mention candidates.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".jj",
    "target",
    "node_modules",
    ".venv",
    "dist",
    "build",
];

/// Scan `root` recursively for files, returning their root-relative paths
/// (forward-slashed), capped at `limit`. Hidden and heavy build dirs are
/// skipped. This is the `@`-file-mention candidate source; it does IO, so it is
/// kept out of the pure detect/filter/apply core above.
pub fn scan_files(root: &std::path::Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(path),
                Ok(ft) if ft.is_file() => {
                    if let Ok(rel) = path.strip_prefix(root) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                        if out.len() >= limit {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_mention() {
        let t = detect("hello @ma", 9).unwrap();
        assert_eq!(t.kind, TriggerKind::File);
        assert_eq!(t.start, 6);
        assert_eq!(t.query, "ma");
    }

    #[test]
    fn at_must_be_word_start() {
        // '@' inside a word (no preceding whitespace) is not a trigger.
        assert_eq!(detect("foo@bar", 7), None);
    }

    #[test]
    fn whitespace_in_query_cancels() {
        assert_eq!(detect("@foo bar", 8), None);
    }

    #[test]
    fn detects_leading_command() {
        let t = detect("/he", 3).unwrap();
        assert_eq!(t.kind, TriggerKind::Command);
        assert_eq!(t.start, 0);
        assert_eq!(t.query, "he");
    }

    #[test]
    fn slash_not_at_start_is_not_command() {
        // A '/' mid-line is not a command (no '@' either) → nothing.
        assert_eq!(detect("a /b", 4), None);
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        let cands = vec!["README.md".to_string(), "src/main.rs".to_string()];
        let got = filter(&cands, "main");
        assert_eq!(got, vec![&"src/main.rs".to_string()]);
    }

    #[test]
    fn apply_file_mention() {
        let trig = detect("see @ma", 7).unwrap();
        let (text, caret) = apply("see @ma", 7, &trig, "src/main.rs");
        assert_eq!(text, "see @src/main.rs ");
        assert_eq!(caret, text.len());
    }

    #[test]
    fn a_span_is_found_without_regard_to_case() {
        assert_eq!(span("src/Main.rs", "main"), Some(4..8));
        assert_eq!(span("README.md", "xyz"), None);
        // An empty query lights nothing: every row would match at zero length,
        // which is a highlight that says only that the list is a list.
        assert_eq!(span("README.md", ""), None);
    }

    /// Lowercasing is not length-preserving in Unicode, so a span found in a
    /// lowercased copy of the string can land off a character boundary in the
    /// original — and slicing there is a panic rather than a wrong answer. The
    /// haystack is a filename somebody else chose, so this arrives eventually.
    #[test]
    fn a_span_lands_on_character_boundaries() {
        let name = "İstanbul-notes.md";
        let at = span(name, "notes").expect("the ascii run is found");
        assert_eq!(&name[at.clone()], "notes");
        assert!(name.is_char_boundary(at.start) && name.is_char_boundary(at.end));
    }

    #[test]
    fn folders_are_counted_from_the_files_beneath_them() {
        let files = vec![
            "crates/app/src/main.rs".to_string(),
            "crates/app/src/chat.rs".to_string(),
            "crates/core/src/lib.rs".to_string(),
            "README.md".to_string(),
        ];
        let got = folders(&files);
        // Every ancestor counts the whole subtree, not the files sitting
        // directly in it: mentioning a folder offers its listing.
        assert_eq!(
            got,
            vec![
                ("crates".to_string(), 3),
                ("crates/app".to_string(), 2),
                ("crates/app/src".to_string(), 2),
                ("crates/core".to_string(), 1),
                ("crates/core/src".to_string(), 1),
            ]
        );
        // A file at the root contributes no folder at all.
        assert!(!got.iter().any(|(dir, _)| dir.is_empty()));
    }

    #[test]
    fn a_mention_leads_with_its_name_and_lights_where_it_matched() {
        let files = vec!["crates/app/src/composer.rs".to_string()];
        let (rows, held) = mentions(&files, &[], &[], "compos", 10);
        assert_eq!(held, 0);
        let row = &rows[0];
        assert_eq!(row.name, "composer.rs");
        assert_eq!(row.parent.as_deref(), Some("crates/app/src"));
        assert_eq!(row.name_span, Some(0..6));
        // The query was found in the name, so the parent stays quiet: one run
        // of characters lit twice reads as two matches.
        assert_eq!(row.parent_span, None);
        assert_eq!(row.insert, "crates/app/src/composer.rs");
    }

    /// A row has one line and paths are longer than it, so what gets cut has to
    /// be the folder and never the filename — that is the one piece of the path
    /// the query was typed against. A path with no folder, and one that ends in
    /// a separator, both still lead with something.
    #[test]
    fn a_path_always_leads_with_something_to_read() {
        let files = vec!["README.md".to_string(), "crates/app/".to_string()];
        let (rows, _) = mentions(&files, &[], &[], "", 10);
        assert_eq!(
            (rows[0].name.as_str(), rows[0].parent.as_deref()),
            ("README.md", None)
        );
        assert_eq!(
            (rows[1].name.as_str(), rows[1].parent.as_deref()),
            ("crates/app/", None),
            "a trailing separator leaves no name to split off, so the whole path leads"
        );
    }

    #[test]
    fn a_query_that_only_occurs_in_the_folder_lights_the_folder() {
        let files = vec!["crates/app/src/main.rs".to_string()];
        let (rows, _) = mentions(&files, &[], &[], "app", 10);
        assert_eq!(rows[0].name_span, None);
        assert_eq!(rows[0].parent_span, Some(7..10));
    }

    #[test]
    fn a_folder_offers_a_listing_and_says_how_big_it_is() {
        let files = vec!["docs/a.md".to_string(), "docs/b.md".to_string()];
        let dirs = folders(&files);
        let (rows, _) = mentions(&[], &dirs, &[], "docs", 10);
        assert_eq!(rows[0].kind, MentionKind::Folder);
        // The trailing slash is what makes accepting this a listing rather
        // than a read of every file under it.
        assert_eq!(rows[0].insert, "docs/");
        assert_eq!(rows[0].note.as_deref(), Some("2 files"));

        let one = vec!["docs/a.md".to_string()];
        let (rows, _) = mentions(&[], &folders(&one), &[], "docs", 10);
        assert_eq!(rows[0].note.as_deref(), Some("1 file"));
    }

    /// The whole point of a per-group cap. Sharing one budget lets the first
    /// group spend it, and the group most likely to hold what the user is
    /// reaching for — the thing that just happened — is the one listed last.
    #[test]
    fn a_large_file_list_cannot_crowd_out_the_artifacts() {
        let files: Vec<String> = (0..200).map(|i| format!("src/file{i}.rs")).collect();
        let artifacts = vec!["src/broken.rs".to_string()];
        let (rows, held) = mentions(&files, &[], &artifacts, "", 5);
        assert_eq!(rows.len(), 6, "five files and the one artifact");
        assert_eq!(rows.last().unwrap().kind, MentionKind::Artifact);
        assert_eq!(
            held, 195,
            "what was held back is counted, not dropped quietly"
        );
    }

    /// What a row *says* and what it *inserts* are different strings now, and
    /// the gap between them is where an accepted mention goes wrong: a row
    /// reading `composer.rs` has to put a whole path in, and a folder row has
    /// to add a separator that appears nowhere in what it says.
    #[test]
    fn what_a_row_inserts_is_the_path_and_not_the_name_it_shows() {
        let files = vec!["crates/app/src/composer.rs".to_string()];
        let dirs = folders(&files);
        let (rows, _) = mentions(&files, &dirs, &[], "app", 10);

        let file = rows.iter().find(|r| r.kind == MentionKind::File).unwrap();
        assert_eq!(file.name, "composer.rs");
        let (text, caret) = apply("see @app", 8, &detect("see @app", 8).unwrap(), &file.insert);
        assert_eq!(text, "see @crates/app/src/composer.rs ");
        assert_eq!(caret, text.len());

        let folder = rows.iter().find(|r| r.kind == MentionKind::Folder).unwrap();
        let (text, _) = apply(
            "see @app",
            8,
            &detect("see @app", 8).unwrap(),
            &folder.insert,
        );
        assert_eq!(text, "see @crates/app/ ");
    }

    #[test]
    fn a_summary_is_the_title_a_long_description_opens_with() {
        // The dominant shape: a title sentence, then the prose written for a
        // model rather than for a menu.
        assert_eq!(
            summary("Test-driven development. Use when the user wants to build features test-first, mentions red-green-refactor, or wants integration tests."),
            "Test-driven development"
        );
        // A single sentence keeps all of itself and loses only its full stop.
        assert_eq!(
            summary("Summarise the conversation so far."),
            "Summarise the conversation so far"
        );
        assert_eq!(
            summary("Review the working tree"),
            "Review the working tree"
        );
        // A semicolon in a first sentence is already joining two statements.
        assert_eq!(summary("Run the audit; report only"), "Run the audit");
        // A comma is not a cut: a first sentence with one in it is ordinary
        // prose, and cutting there yields a fragment rather than a title.
        assert_eq!(
            summary("What this conversation has cost, in tokens"),
            "What this conversation has cost, in tokens"
        );
        // A colon is not either — what follows it is usually the content.
        assert_eq!(
            summary("Triggers on: chart, graph"),
            "Triggers on: chart, graph"
        );
        assert_eq!(summary(""), "");
    }

    fn cmd(name: &str, description: &str) -> crate::acp::SlashCommand {
        crate::acp::SlashCommand {
            name: name.to_string(),
            description: description.to_string(),
        }
    }

    #[test]
    fn a_namespace_is_taken_off_the_row_and_kept_for_the_heading() {
        let all = vec![cmd("ponytail:audit", "Audit the repo. Use when asked.")];
        let (rows, _) = commands(&all, "", 10);
        assert_eq!(rows[0].namespace.as_deref(), Some("ponytail"));
        assert_eq!(
            rows[0].name, "audit",
            "the prefix is off the front of the name"
        );
        assert_eq!(
            rows[0].insert, "ponytail:audit",
            "what is sent is still the whole name the agent answers to"
        );
        assert_eq!(rows[0].summary.as_deref(), Some("Audit the repo"));
    }

    #[test]
    fn the_agents_own_commands_lead_the_namespaced_ones() {
        let all = vec![
            cmd("zebra:one", ""),
            cmd("compact", ""),
            cmd("alpha:two", ""),
        ];
        let (rows, _) = commands(&all, "", 10);
        let order: Vec<Option<&str>> = rows.iter().map(|r| r.namespace.as_deref()).collect();
        assert_eq!(order, vec![None, Some("alpha"), Some("zebra")]);
    }

    /// Typing the namespace still has to find its commands — but the characters
    /// it matched are no longer drawn on the row, so lighting a range into the
    /// name would light the wrong letters. The heading is what says why the
    /// rows are there.
    #[test]
    fn a_query_matching_only_the_prefix_finds_the_row_and_lights_nothing() {
        let all = vec![cmd("ponytail:audit", "")];
        let (rows, _) = commands(&all, "ponyt", 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name_span, None);

        let (rows, _) = commands(&all, "aud", 10);
        assert_eq!(
            rows[0].name_span,
            Some(0..3),
            "a match in the name still lights"
        );
    }

    #[test]
    fn a_namespace_is_never_said_a_second_time_in_the_summary() {
        let all = vec![cmd("ponytail:audit", "Ponytail: audit the repo for bloat.")];
        let (rows, _) = commands(&all, "", 10);
        assert_eq!(rows[0].summary.as_deref(), Some("audit the repo for bloat"));
    }

    #[test]
    fn a_command_with_no_description_gets_no_second_column() {
        let all = vec![cmd("compact", "   ")];
        let (rows, _) = commands(&all, "", 10);
        assert_eq!(rows[0].summary, None);
    }

    /// The artifact run is short by design, but the cut has to happen *after*
    /// the query and has to be counted. Cut upstream instead, a query aimed at
    /// something older found nothing in this group and the held-back count —
    /// the only thing on screen that ever says a bound bit — could not see it.
    #[test]
    fn an_artifact_older_than_the_run_is_counted_and_still_reachable() {
        let artifacts: Vec<String> = (0..20).map(|i| format!("src/touched{i}.rs")).collect();
        let (rows, held) = mentions(&[], &[], &artifacts, "", 50);
        assert_eq!(rows.len(), ARTIFACT_ROWS, "the run stays short");
        assert_eq!(held, 20 - ARTIFACT_ROWS, "and says how much it is holding");

        // Narrowed onto one the run would not have reached, it is found.
        let (rows, held) = mentions(&[], &[], &artifacts, "touched19", 50);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "touched19.rs");
        assert_eq!(held, 0);
    }

    /// **What lets the popup stand at one height.** It is sized once, from the
    /// list with nothing typed, and then never resized while it is open — which
    /// is only safe because no query can produce more rows, or more groups,
    /// than an empty one. If that ever stopped holding, the popup would clip
    /// rows it had no room for instead of growing, which is a worse failure
    /// than the jitter it replaced.
    #[test]
    fn no_query_can_produce_more_than_an_empty_one() {
        let files: Vec<String> = ["a/one.rs", "a/two.rs", "b/three.md", "readme.md"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let dirs = folders(&files);
        let artifacts = vec!["a/one.rs".to_string()];
        let groups = |rows: &[Mention]| {
            let mut kinds: Vec<MentionKind> = rows.iter().map(|r| r.kind).collect();
            kinds.dedup();
            kinds.len()
        };

        let (all, _) = mentions(&files, &dirs, &artifacts, "", 50);
        for query in ["", "a", "one", "md", "READ", "zzz", "/", "."] {
            let (some, _) = mentions(&files, &dirs, &artifacts, query, 50);
            assert!(
                some.len() <= all.len(),
                "{query:?} produced {} rows against {} unfiltered",
                some.len(),
                all.len()
            );
            assert!(
                groups(&some) <= groups(&all),
                "{query:?} produced more groups than an empty query"
            );
        }

        let all_cmds = vec![
            cmd("compact", ""),
            cmd("ponytail:audit", ""),
            cmd("ponytail:debt", ""),
        ];
        let (every, _) = commands(&all_cmds, "", 50);
        for query in ["", "c", "pony", "audit", "zzz"] {
            let (some, _) = commands(&all_cmds, query, 50);
            assert!(
                some.len() <= every.len(),
                "{query:?} produced more command rows than an empty query"
            );
        }
    }

    #[test]
    fn groups_that_match_nothing_contribute_no_rows() {
        let files = vec!["README.md".to_string()];
        let (rows, held) = mentions(&files, &folders(&files), &[], "zzz", 10);
        assert!(rows.is_empty());
        assert_eq!(held, 0);
    }

    #[test]
    fn apply_command_replaces_whole() {
        let trig = detect("/he", 3).unwrap();
        let (text, caret) = apply("/he", 3, &trig, "help");
        assert_eq!(text, "/help ");
        assert_eq!(caret, "/help ".len());
    }
}
