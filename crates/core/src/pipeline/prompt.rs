//! What a step's prompt becomes once its variables are filled in and onehand
//! has said where the work is and what it will check, and what a turn that
//! failed a gate is told.
//!
//! **A variable is `{name}`**, a lowercase name or `output.<step id>`. Any
//! other brace is text, so a code sample in a prompt needs no escaping. A
//! name onehand does not know is refused when the template is saved, so
//! filling one in never fails.

use super::facts::Facts;
use super::run::Brief;
use super::template::{GateKind, Place};
use std::collections::BTreeMap;

/// The variables every prompt may use besides `output.<id>`.
pub(crate) const NAMES: [&str; 4] = ["brief", "instructions", "check_output", "revise"];

/// Every variable `prompt` names, in order, repeats included.
pub(crate) fn refs(prompt: &str) -> Vec<&str> {
    spans(prompt).into_iter().map(|(_, name)| name).collect()
}

/// Each variable in `prompt` with where it starts.
fn spans(prompt: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let mut rest = 0;
    while let Some(open) = prompt[rest..].find('{').map(|at| rest + at) {
        rest = open + 1;
        let Some(close) = prompt[rest..].find('}').map(|at| rest + at) else {
            break;
        };
        let name = &prompt[open + 1..close];
        if is_name(name) {
            found.push((open, name));
            rest = close + 1;
        }
    }
    found
}

/// `brief`, `check_output`, `output.plan`: a lowercase word, perhaps with
/// one dotted part. Anything else in braces is left as text.
fn is_name(text: &str) -> bool {
    let (head, tail) = match text.split_once('.') {
        Some((head, tail)) => (head, Some(tail)),
        None => (text, None),
    };
    let word = |w: &str, extra: &[char]| {
        !w.is_empty()
            && w.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || extra.contains(&c)
            })
    };
    head.starts_with(|c: char| c.is_ascii_lowercase())
        && word(head, &[])
        && tail.is_none_or(|tail| word(tail, &['-']))
}

/// What a step's prompt is filled in with.
pub(crate) struct Fill<'a> {
    pub(crate) brief: &'a Brief,
    pub(crate) outputs: &'a BTreeMap<String, String>,
    pub(crate) check_output: Option<&'a str>,
    pub(crate) revise: Option<&'a str>,
    /// What the step being revised answered last time, said beside the note.
    pub(crate) revised_answer: Option<&'a str>,
}

/// The prompt that starts a step: `prompt` filled in, then what onehand
/// adds — what the person asked of every step, a failed check or a note on
/// a revision the prompt does not place itself, where the work is, and the
/// rule each gate checks.
pub(crate) fn step_prompt(prompt: &str, gates: &[GateKind], place: Place, fill: &Fill) -> String {
    let named = refs(prompt);
    let mut text = String::new();
    let mut at = 0;
    for (open, name) in spans(prompt) {
        text.push_str(&prompt[at..open]);
        text.push_str(&value(name, fill));
        at = open + name.len() + 2;
    }
    text.push_str(&prompt[at..]);
    let mut text = text.trim().to_string();

    let mut add = |part: String| {
        text.push_str("\n\n");
        text.push_str(&part);
    };
    if let Some(extra) = fill.brief.instructions.as_deref() {
        if !named.contains(&"instructions") {
            add(format!(
                "Instructions from the person who started this:\n\n{}",
                quoted(extra)
            ));
        }
    }
    if let Some(out) = fill
        .check_output
        .filter(|_| !named.contains(&"check_output"))
    {
        add(format!("The check failed:\n\n```\n{}\n```", out.trim()));
    }
    if let Some(note) = fill.revise.filter(|_| !named.contains(&"revise")) {
        let before = fill
            .revised_answer
            .map(|answer| format!("\n\nWhat you answered before:\n\n{}", quoted(answer)))
            .unwrap_or_default();
        add(format!(
            "A person read your last answer to this step and asked for changes:\n\n{}{before}",
            quoted(note)
        ));
    }
    add("---".to_string());
    add(place_said(place).to_string());
    add(
        "Read the repository's own agent instructions, and whatever they point at, and \
         follow its conventions. Decide whatever the code, the tests and the documentation \
         let you infer, and list the assumptions that mattered in your answer. Only a \
         decision they cannot settle, about what the product should do, is for a person: \
         ask it with your tool for asking the user a question, not in your answer, and \
         carry on once it is answered. Do not guess at those."
            .to_string(),
    );
    let rules: Vec<String> = gates
        .iter()
        .map(|gate| format!("- {}", rule(*gate, place)))
        .collect();
    if !rules.is_empty() {
        add(format!(
            "onehand checks this step itself when your turn ends:\n\n{}",
            rules.join("\n")
        ));
    }
    text.push('\n');
    text
}

/// What the variable `name` stands for.
fn value(name: &str, fill: &Fill) -> String {
    match name {
        "brief" => {
            let body = fill.brief.body.trim();
            match body.is_empty() {
                true => format!("Title: {}", fill.brief.title.trim()),
                false => format!("Title: {}\n\n{body}", fill.brief.title.trim()),
            }
        }
        "instructions" => fill.brief.instructions.clone().unwrap_or_default(),
        "check_output" => fill.check_output.unwrap_or_default().trim().to_string(),
        "revise" => fill.revise.unwrap_or_default().to_string(),
        _ => name
            .strip_prefix("output.")
            .and_then(|id| fill.outputs.get(id))
            .cloned()
            .unwrap_or_default(),
    }
}

/// Where the work happens, as the agent is told it.
fn place_said(place: Place) -> &'static str {
    match place {
        Place::Checkout => {
            "You are in this checkout, on whatever branch it has. A person has it open and \
             may be working in it too. Do not create a branch or a worktree, and do not \
             switch branches."
        }
        Place::Worktree => {
            "You are on a branch of your own, in a worktree of its own. Do not push: onehand \
             does not take the work further than the branch."
        }
    }
}

/// What the agent is told a gate checks.
fn rule(gate: GateKind, place: Place) -> &'static str {
    match (gate, place) {
        (GateKind::Answered, _) => {
            "Your answer is this step's result: end the turn with it, in full."
        }
        (GateKind::CodeUnchanged, Place::Checkout) => {
            "Do not edit any file or commit: a turn that changed the checkout is sent back."
        }
        (GateKind::CodeUnchanged, Place::Worktree) => {
            "Do not edit any file or commit: a turn that changed the branch is sent back."
        }
        (GateKind::CodeChanged, _) => "The turn has to change the code.",
        (GateKind::Committed, _) => {
            "Commit your work on this branch before the turn ends, leaving nothing uncommitted."
        }
        (GateKind::Uncommitted, _) => {
            "Leave your changes uncommitted for the person who started this: do not commit."
        }
    }
}

/// The prompt that sends a session back to work after `gate` failed, given
/// what was read of the work.
pub(crate) fn carry_on(gate: GateKind, facts: &Facts, place: Place) -> String {
    let (looked_at, finish, put_back) = match place {
        Place::Checkout => (
            "the checkout",
            "leave it uncommitted in this checkout",
            // A person may be working in a checkout too, so what the gate saw
            // change may be theirs: the agent undoes only its own.
            "Undo any edit or commit you made, and leave every change that is not yours",
        ),
        Place::Worktree => (
            "the branch",
            "commit it on this branch",
            "Put the worktree and the branch back as they were",
        ),
    };
    let said = match gate {
        GateKind::Answered => {
            "that turn gave no answer. Answer with the result itself.".to_string()
        }
        GateKind::CodeUnchanged => format!(
            "that turn changed the code, and this step is not to. {put_back}, then answer \
             again, changing nothing."
        ),
        GateKind::CodeChanged => format!(
            "that turn changed nothing, so the step is not done. Carry on with it, then \
             {finish}."
        ),
        GateKind::Committed if facts.dirty => {
            format!("the worktree holds changes no commit has. Finish the work, then {finish}.")
        }
        GateKind::Committed => {
            format!(
                "the branch has no new commit, so the step is not done. Carry on, then {finish}."
            )
        }
        GateKind::Uncommitted => "a commit landed during that turn, and the work here is to \
             stay uncommitted for the person who started it. If you made it, undo it, keeping \
             its changes in the checkout, and do not commit again; leave any commit that is not \
             yours."
            .to_string(),
    };
    format!("onehand checked {looked_at} after that turn: {said}")
}

/// `text` as a Markdown quote, so a person's words stand apart from onehand's.
fn quoted(text: &str) -> String {
    text.trim()
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
