//! Build-time guards for rules the compiler cannot express.
//!
//! Every test here exists because the same mistake was made more than once.
//! That is the bar: a guard for a hypothetical is noise, and noise is what gets
//! a test file deleted.
//!
//! **Part of this job is not done here.** Making the crate's internal modules
//! private (see [`crate`]) lets rustc's own `dead_code` analysis reach them,
//! and doing that found five dead items the moment it was switched on. But
//! `dead_code` counts an *assignment* as a use, so it says nothing about a
//! field that is written and never read — which is the shape that actually kept
//! recurring. [`tests::no_field_is_written_and_never_read`] covers as much of
//! that as a source scan honestly can; its own doc says where it stops.

/// Every `.rs` file under this crate's `src/`, as (path, source).
///
/// Read at run time rather than `include_str!`: the point is to catch a rule
/// broken in a file nobody thought to list, and a hard-coded list is a list
/// that goes stale the first time someone adds a module.
#[cfg(test)]
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push((path.display().to_string(), text));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &mut out);
    assert!(!out.is_empty(), "found no sources under {}", root.display());
    out
}

/// Every `.rs` file in **both** first-party crates.
///
/// [`sources`] is deliberately this crate only, because the rules it feeds are
/// this crate's. The flag guard below needs a wider net: a field can be
/// declared in `onehand-core` and only ever assigned from `onehand`, which is
/// exactly how `EditorFile::dirty` hid — a scan of either crate alone sees half
/// the story and concludes nothing is wrong.
#[cfg(test)]
fn workspace_sources() -> Vec<(String, String)> {
    let mut out = sources();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .join("core")
        .join("src");
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push((path.display().to_string(), text));
            }
        }
    }
    walk(&core, &mut out);
    assert!(
        out.iter().any(|(p, _)| p.contains("core")),
        "found no sources under {}",
        core.display()
    );
    out
}

/// Every `.md` file in the repository, as (path, text).
///
/// Walks up from this crate to the workspace root, because the documents that
/// matter most sit beside `Cargo.toml` rather than inside a crate. `target/` is
/// skipped: prose that arrived with a downloaded dependency was written by
/// somebody else and is not ours to hold to anything.
#[cfg(test)]
fn documents() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Build output and version-control internals hold other
                // people's prose in whatever language they wrote it.
                if name == "target" || name == ".git" {
                    continue;
                }
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push((path.display().to_string(), text));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root");
    let mut out = Vec::new();
    walk(root, &mut out);
    assert!(
        out.iter().any(|(p, _)| p.ends_with("CLAUDE.md")),
        "found no documents under {}",
        root.display()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{documents, sources, workspace_sources};

    /// Glyphs that have an `IconName` equivalent and must never be typed as
    /// text. The rule: *every icon is an SVG.*
    ///
    /// It is not pedantry. A glyph renders in whatever font the run happens to
    /// resolve, sits on the text baseline instead of the row's centre, ignores
    /// the icon size scale, and silently becomes a blank box on a machine
    /// missing the coverage. Nine of these had accumulated by the time anyone
    /// looked, because the icon set has a consistency test and
    /// this rule had nothing at all.
    const GLYPHS: &[(char, &str)] = &[
        ('✕', "IconName::Close"),
        ('✖', "IconName::Close"),
        ('×', "IconName::Close"),
        ('✓', "IconName::Check"),
        ('✔', "IconName::Check"),
        ('●', "a div with .rounded_full()"),
        ('○', "a div with .rounded_full()"),
        ('•', "a div with .rounded_full()"),
        ('▸', "IconName::ChevronRight"),
        ('▾', "IconName::ChevronDown"),
        ('❯', "IconName::ChevronRight"),
        ('⚙', "IconName::Settings"),
        ('＋', "IconName::Plus"),
    ];

    #[test]
    fn no_glyph_is_used_as_an_icon() {
        for (path, source) in sources() {
            for (glyph, instead) in GLYPHS {
                // Quoted, so prose in a doc comment can still name the glyph it
                // is explaining -- which these very comments do.
                let literal = format!("\"{glyph}\"");
                assert!(
                    !source.contains(&literal),
                    "{path} renders {literal} as text; use {instead} \
                     (every icon is an SVG from the registry)"
                );
            }
        }
    }

    /// Every document in the repository is written in English.
    ///
    /// These files are the binding contracts, they are read alongside source
    /// that is entirely in English, and much of what they explain is quoted
    /// identifiers and compiler output that has no translation. A repository
    /// split across two languages is one whose documents get read in neither:
    /// the reader has to switch, and the terms stop matching the code they
    /// name. One file was in Vietnamese for months while the four beside it
    /// were not, and nothing said so.
    ///
    /// Detected by Vietnamese-specific characters rather than by trying to
    /// identify a language, which a test has no business attempting. The
    /// precomposed diacritics below appear in no English text and in no
    /// identifier, so a hit is unambiguous — while `đ`, and the plain vowels
    /// that Vietnamese shares with English, are deliberately absent.
    #[test]
    fn documents_are_written_in_english() {
        // Written as escapes so this test's own source does not trip the rule
        // it enforces, and so the list survives an editor that normalizes
        // Unicode on save.
        const MARKS: &[char] = &[
            '\u{1ea1}', // ạ
            '\u{1ea3}', // ả
            '\u{1eaf}', // ắ
            '\u{1ea7}', // ầ
            '\u{1ebf}', // ế
            '\u{1ec7}', // ệ
            '\u{1ec9}', // ỉ
            '\u{1ed1}', // ố
            '\u{1ed9}', // ộ
            '\u{1edd}', // ờ
            '\u{1ee3}', // ợ
            '\u{1ee9}', // ứ
            '\u{1ef1}', // ự
            '\u{1ef5}', // ỵ
        ];
        for (path, text) in documents() {
            for (n, line) in text.lines().enumerate() {
                let Some(mark) = MARKS.iter().find(|m| line.contains(**m)) else {
                    continue;
                };
                panic!(
                    "{path}:{}: documents are written in English, and this line \
                     is not (found {mark:?}).\n    {}",
                    n + 1,
                    line.trim()
                );
            }
        }
    }

    /// Status fills are not status ink.
    ///
    /// `danger`, `warning`, `success`, and `info` are backgrounds with paired
    /// foreground tokens for content placed on them. They were repeatedly used
    /// as coloured text on the normal app background instead, which only had
    /// enough contrast in the dark palette. App-owned status text and icons go
    /// through `crate::theme::status_ink` so their hue is pulled toward the
    /// active theme's foreground.
    #[test]
    fn status_backgrounds_are_not_used_as_direct_text_colors() {
        let forbidden = ["danger", "warning", "success", "info"];
        for (path, source) in sources() {
            for role in forbidden {
                for receiver in ["cx.theme()", "theme"] {
                    let expression = format!(".text_color({receiver}.{role})");
                    assert!(
                        !source.contains(&expression),
                        "{path} uses the status background `{role}` as text; use \
                         crate::theme::status_ink instead"
                    );
                }
            }
        }
    }

    /// No field is assigned and never read back.
    ///
    /// This is the shape that kept recurring: `Shell.status` first, then
    /// `Shell.layout_dirty` and `ChatPane.unseen` written again after that one
    /// was fixed. A field nothing reads is indistinguishable
    /// from a feature that works — the code that sets it looks exactly right.
    ///
    /// **rustc will not do this for us.** Its `dead_code` lint counts an
    /// assignment as a use, so `self.f = x` with no reader anywhere warns about
    /// nothing, private module or not. (It *does* catch a field that is never
    /// mentioned again after construction, which is worth having and is why the
    /// modules are private.)
    ///
    /// **Where this stops.** It reads `.name` occurrences and treats anything
    /// that is not an assignment target as a read — so a field mutated through
    /// its own methods (`self.set.insert(..)`, `self.map.remove(..)`) counts as
    /// read even when the collection's *contents* are never observed. That is
    /// exactly how `ChatPane.unseen` hid, and this test would not have caught
    /// it: telling mutation from observation needs the type, which a source
    /// scan does not have. It catches the scalar case, which is the other half.
    ///
    /// A field that is deliberately held rather than read — an RAII guard whose
    /// whole job is its `Drop` — opts out by starting with `_`, the convention
    /// this codebase already uses for `_pump`, `_pty`, `_child`.
    #[test]
    fn no_field_is_written_and_never_read() {
        let sources = sources();
        // Comment lines are dropped before the scan. Prose naming a field --
        // `ChatPane.unseen`, say -- reads as `.unseen` to a source scan, so a
        // doc comment explaining that a field went unread was itself enough to
        // make it look read. This test failed to catch its own example that
        // way, which is a fair warning about how far a source scan can be
        // trusted.
        let whole: String = sources
            .iter()
            .flat_map(|(_, s)| s.lines())
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for (path, source) in &sources {
            for field in struct_fields(source) {
                if field.starts_with('_') {
                    continue;
                }
                let reads = whole
                    .match_indices(&format!(".{field}"))
                    .filter(|(i, _)| {
                        // `.name` followed by `=` (but not `==`) is a write.
                        let after = &whole[i + field.len() + 1..];
                        let rest = after.trim_start();
                        !rest.starts_with('=') || rest.starts_with("==")
                    })
                    // Guard against a prefix match: `.git` inside `.gitstat`.
                    .filter(|(i, _)| {
                        whole[i + field.len() + 1..]
                            .chars()
                            .next()
                            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                    })
                    .count();

                assert!(
                    reads > 0,
                    "{path}: `{field}` is assigned and never read. Either something \
                     should be reading it, or it should not exist. If it is held \
                     purely for its `Drop`, name it `_{field}`."
                );
            }
        }
    }

    /// No `bool` field is only ever assigned one literal.
    ///
    /// The other half of [`no_field_is_written_and_never_read`], and the half
    /// that was missing. That test asks "does anything read this?"; a flag can
    /// pass it and still be meaningless, because the thing that never happens
    /// is the *other* assignment.
    ///
    /// `EditorFile::dirty` is the case. Its three appearances were: declared
    /// with a doc comment describing the amber tab dot, initialised `false`,
    /// and set `false` again after a save. Nothing anywhere set it `true`, so
    /// the dot was unreachable and every tab close discarded unsaved edits in
    /// silence. Every individual line looked correct; only the
    /// *set* of assignments was wrong, which is not a shape rustc has an
    /// opinion about and not one a reviewer notices across two crates.
    ///
    /// **Only the `false` direction is checked**, and that asymmetry is the
    /// point. A flag that is only ever assigned `true` is a *latch* -- `false`
    /// by construction, raised once, never lowered -- and core has two correct
    /// ones (`Buffer::truncated`, `ExitState::exited`). A flag that is only
    /// ever assigned `false` has no such reading: whatever is supposed to raise
    /// it does not exist, and the code that clears it looks perfectly right,
    /// which is why nobody sees it.
    ///
    /// **Where this stops.** Only `bool` fields, and a field built `true` by a
    /// hand-written `Default` and latched off would trip it -- there is none
    /// today, and the message says which field to look at.
    #[test]
    fn no_flag_is_only_ever_assigned_false() {
        let sources = workspace_sources();
        let whole: String = sources
            .iter()
            .flat_map(|(_, s)| s.lines())
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for (path, source) in &sources {
            for field in bool_fields(source) {
                // Every `.name = <rhs>` in either crate.
                let assigned: std::collections::HashSet<&str> = whole
                    .match_indices(&format!(".{field} = "))
                    .map(|(i, _)| {
                        whole[i + field.len() + 4..]
                            .split([';', ',', '\n'])
                            .next()
                            .unwrap_or_default()
                            .trim()
                    })
                    .collect();

                if assigned != std::collections::HashSet::from(["false"]) {
                    continue;
                }
                // A `name: true,` somewhere means it starts raised and this is
                // the thing that lowers it -- a latch the other way up.
                if whole.contains(&format!("{field}: true,")) {
                    continue;
                }
                panic!(
                    "{path}: `{field}` is only ever assigned `false`, and nothing \
                     ever sets it `true`. Either something should be raising it, \
                     or it should not exist ."
                );
            }
        }
    }

    /// Source files name no document.
    ///
    /// A comment explains the code in its own words. It does not point at a
    /// markdown file and a section, because that pointer decays the moment the
    /// file is reorganized -- and a confidently wrong pointer is worse than no
    /// pointer, since the reader goes and looks. It also lets a comment gesture
    /// at an explanation instead of giving one.
    ///
    /// The traffic runs one way: the documents point at the code.
    ///
    /// **What is still allowed** is a reference to *code* -- an intra-doc link,
    /// a module path, a file and line, an upstream revision. Those are
    /// checkable, and rustdoc breaks the build when an intra-doc link rots.
    ///
    /// This existed as a convention and decayed exactly as predicted: 124
    /// citations had accumulated across both crates before anyone counted.
    ///
    /// **A file name is the rarest way to cite one.** For a long time this
    /// checked for the name plus its extension and nothing else, so it passed
    /// with thirty-one live citations sitting in front of it -- the section
    /// marks and the short item codes the documents number their decisions and
    /// findings with, which is how anyone actually writes the pointer. All
    /// three forms rot the same way and all three are caught here now.
    /// Every button in the app answers the pointer.
    ///
    /// The component library draws all of its button variants but `link` and
    /// `text` with the arrow cursor, while everything this app hand-rolls as a
    /// row, chip or tab shows a pointer. Half of the actions on screen
    /// answering the cursor and half not is worse than either rule applied
    /// whole: the pointer stops meaning anything, and the only way left to find
    /// out whether something is clickable is to click it.
    ///
    /// So the wrapper is the only way a button is built here, and this is what
    /// keeps it that way -- the wrapper is one line to bypass and the bypass
    /// looks exactly like ordinary code.
    #[test]
    fn every_button_goes_through_the_app_wrapper() {
        // Assembled at run time so the wrapper's own source, and this test,
        // are not matches for it.
        let bare = format!("Button{}new(", "::");
        for (path, source) in sources() {
            // The one file allowed to name it: the wrapper is where the
            // library's control is reached.
            if path.ends_with("controls.rs") {
                continue;
            }
            for (n, line) in source.lines().enumerate() {
                assert!(
                    !line.contains(&bare),
                    "{path}:{}: builds a button straight from the library, which \
                     draws it with the arrow cursor. Use the app's action \
                     wrapper so it answers the pointer like every other control.\n    {}",
                    n + 1,
                    line.trim()
                );
            }
        }
    }

    #[test]
    fn code_never_cites_a_document() {
        // Assembled from stems at run time so this test does not match its own
        // source and fail on itself. The section mark is written as an escape
        // for that same reason.
        //
        // Longer than the set of documents that currently exists, on purpose.
        // Several of these name write-ups that have since been deleted, and a
        // retired name is precisely the one a stale comment would still be
        // pointing at -- a rule that only forbids live names stops catching a
        // citation at the moment it becomes worthless.
        const STEMS: &[&str] = &[
            "CLAUDE",
            "DESIGN",
            "DESIGN-ANSWER",
            "DECISIONS",
            "AUDIT",
            "AUDIT-2",
            "MIGRATION-GPUI",
            "REFACTOR-CHAT-PANE",
            "UI-UX-PROPOSAL",
            "README",
        ];
        const SECTION_MARK: char = '\u{a7}';

        for (path, source) in workspace_sources() {
            for (n, line) in source.lines().enumerate() {
                // A test fixture may legitimately hold a file name as data;
                // what the rule forbids is prose pointing at a document.
                if !line.trim_start().starts_with("//") {
                    continue;
                }
                let cited = STEMS
                    .iter()
                    .find(|stem| line.contains(**stem))
                    .map(|stem| (*stem).to_string())
                    .or_else(|| {
                        line.contains(SECTION_MARK)
                            .then(|| SECTION_MARK.to_string())
                    })
                    .or_else(|| item_code(line));
                let Some(cited) = cited else {
                    continue;
                };
                panic!(
                    "{path}:{}: cites `{cited}`. Code describes, it never cites -- \
                     write the reason out in the comment's own words instead.\n    {}",
                    n + 1,
                    line.trim()
                );
            }
        }
    }

    /// An archived conversation is adopted in exactly one place.
    ///
    /// The model's loader takes items and nothing else; the *parsed* markdown
    /// those items need is the front end's, and it lives on the session. Call
    /// the loader directly and the transcript comes up drawing raw markdown
    /// source — every heading, fence and asterisk of the conversation — until
    /// some later event syncs the cache by accident. That is what
    /// [`crate::chat::session::ChatSession::adopt`] exists to make impossible,
    /// and a guard is the only thing that keeps it that way: the wrong call is
    /// one line shorter and looks perfectly reasonable.
    #[test]
    fn an_archived_conversation_is_adopted_in_one_place() {
        // Assembled at run time so this test does not match its own source.
        let loader = format!("resume_{}(", "from");
        let callers: Vec<String> = sources()
            .into_iter()
            .filter(|(_, source)| {
                source
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("//"))
                    .any(|line| line.contains(&loader))
            })
            .map(|(path, _)| path)
            .collect();
        assert_eq!(
            callers.len(),
            1,
            "`{loader}` is called from {callers:?}. Loading an archive is the \
             half that has to parse it -- go through `ChatSession::adopt`, \
             which does both."
        );
        assert!(
            callers[0].ends_with("chat/session.rs"),
            "`{loader}` moved to {}; the call belongs with the markdown cache \
             it has to fill",
            callers[0]
        );
    }

    /// The first item code in `line`, if it holds one.
    ///
    /// A code is a capital `D` or `P` (the decision and finding registers),
    /// then digits, optionally a dash and one more capital: the shape of a
    /// pointer into a numbered list somebody keeps somewhere else. Bounded to
    /// those two letters on purpose -- a wider net would catch `H264` and every
    /// other capital-then-digits token that means itself.
    fn item_code(line: &str) -> Option<String> {
        line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .find(|word| {
                let mut chars = word.chars();
                if !matches!(chars.next(), Some('D' | 'P')) {
                    return false;
                }
                let rest: String = chars.collect();
                let (digits, suffix) = match rest.split_once('-') {
                    Some((digits, suffix)) => (digits, Some(suffix)),
                    None => (rest.as_str(), None),
                };
                !digits.is_empty()
                    && digits.chars().all(|c| c.is_ascii_digit())
                    && suffix
                        .is_none_or(|s| s.len() == 1 && s.chars().all(|c| c.is_ascii_uppercase()))
            })
            .map(str::to_string)
    }

    /// Names of `bool`-typed fields declared in one file.
    ///
    /// Comparison-deriving structs are *not* skipped here, unlike in the
    /// read-back scan: a flag that is only ever assigned `false` is meaningless
    /// whether or not something compares the struct it sits in.
    fn bool_fields(source: &str) -> Vec<String> {
        struct_fields_typed(source, false)
            .into_iter()
            .filter(|(_, ty)| ty == "bool")
            .map(|(name, _)| name)
            .collect()
    }

    /// Field names declared in `struct X { .. }` blocks in one file, minus the
    /// ones a derive already reads.
    fn struct_fields(source: &str) -> Vec<String> {
        struct_fields_typed(source, true)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// Field names *and* their declared types.
    ///
    /// With `skip_compared`, fields of a struct that derives an equality or
    /// ordering trait are left out. The generated comparison reads every field,
    /// so a cache key whose whole job is to be compared has no `.field` site
    /// anywhere -- and reporting it as dead is the scan describing its own
    /// blind spot rather than a problem in the code.
    fn struct_fields_typed(source: &str, skip_compared: bool) -> Vec<(String, String)> {
        const COMPARED: &[&str] = &["PartialEq", "Eq", "Hash", "PartialOrd", "Ord"];
        let mut out = Vec::new();
        let mut depth = 0usize;
        let mut in_struct = false;
        let mut compared = false;
        let mut derives = String::new();
        for line in source.lines() {
            let trimmed = line.trim_start();
            if !in_struct {
                if trimmed.starts_with("#[") {
                    derives.push_str(trimmed);
                    continue;
                }
                // `struct X {` -- but not a tuple struct or a `struct X;`.
                if trimmed.contains("struct ") && trimmed.ends_with('{') {
                    in_struct = true;
                    depth = 1;
                    compared = COMPARED.iter().any(|t| derives.contains(t));
                }
                derives.clear();
                continue;
            }
            depth += line.matches('{').count();
            depth -= line.matches('}').count().min(depth);
            if depth == 0 {
                in_struct = false;
                continue;
            }
            if compared && skip_compared {
                continue;
            }
            // Only the field lines themselves, at the struct's own level.
            let decl = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
            let Some((name, ty)) = decl.split_once(':') else {
                continue;
            };
            let name = name.trim();
            let ty = ty.trim().trim_end_matches(',').trim();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && trimmed.ends_with(',')
            {
                out.push((name.to_string(), ty.to_string()));
            }
        }
        out
    }

    /// This crate's own event enums must be consumed by an exhaustive `match`.
    ///
    /// `if matches!(event, Foo::Bar)` compiles forever. Add a variant and every
    /// such site silently ignores it — which is exactly how
    /// `ChatEvent::OpenFile` was emitted for a whole phase with nothing on the
    /// other end, so clicking a path in a tool card did nothing.
    /// A `match` turns that into a build failure at every consumer.
    ///
    /// Scoped to enums declared **here**. `matches!` on a library enum
    /// (`DockEvent`, `InputEvent`) is correct and stays allowed: those grow
    /// variants on their own schedule, and matching them exhaustively would
    /// break the build on every dependency bump for no safety at all.
    #[test]
    fn our_event_enums_are_matched_exhaustively() {
        let sources = sources();

        let mut ours: Vec<String> = Vec::new();
        for (_, source) in &sources {
            for line in source.lines() {
                let line = line.trim_start();
                let Some(rest) = line.strip_prefix("pub enum ") else {
                    continue;
                };
                let name = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or_default();
                if name.ends_with("Event") {
                    ours.push(name.to_string());
                }
            }
        }
        assert!(
            !ours.is_empty(),
            "no `*Event` enum found -- this guard has stopped guarding anything"
        );

        for (path, source) in &sources {
            for name in &ours {
                for (i, _) in source.match_indices("matches!(") {
                    let tail = &source[i..];
                    let end = tail.find(')').map(|e| i + e).unwrap_or(source.len());
                    let call = &source[i..end];
                    assert!(
                        !call.contains(&format!("{name}::")),
                        "{path} tests {name} with `matches!`; use an exhaustive \
                         `match` so a new variant is a compile error here \
                         "
                    );
                }
            }
        }
    }
}
