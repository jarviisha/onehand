//! What a line typed into a remote chat is asking for.
//!
//! Pure, and in core rather than beside the channel that received it, because a
//! second channel would otherwise grow a second parser — and two parsers of the
//! same little language disagree at the edges long before anybody notices.

/// One line, understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommand {
    /// `/sessions` — every session this process is running.
    List,
    /// `/help` — what this chat can do.
    Help,
    /// `/use <n>` — point this chat at the session numbered `n`.
    ///
    /// The number is the session's own id as the listing prints it, not its
    /// place in the list. A place would be the wrong handle for a chat to hold:
    /// the list is read, a message is typed, and a session closed in between
    /// would slide every place below it onto a different conversation.
    Use(u64),
    /// `/use` with nothing to act on — no number, or a word that is not one.
    UseWhich,
    /// `/archive` — the conversations on disk, across every project.
    Archive,
    /// `/open <n>` — reopen the n-th conversation of the last [`Self::Archive`]
    /// listing.
    ///
    /// **A place in that listing, not an identity.** A saved conversation is
    /// named on disk by an agent-chosen session id, which is far too long to
    /// type on a phone and too long for a button to carry — so what a chat holds
    /// is a number, and what makes the number safe is that the listing it counts
    /// into is kept exactly as it was sent rather than read off disk again.
    Open(usize),
    /// `/open` with nothing to act on — no number, or a word that is not one.
    OpenWhich,
    /// `/away` — stop assuming the window can be seen.
    ///
    /// The app decides what a notification is worth by asking whether the user
    /// is looking at the thing it is about. That question has one answer it
    /// cannot work out on its own: somebody who has walked away leaves a window
    /// that is still focused, still showing the conversation, and in front of an
    /// empty chair. This says so.
    Away,
    /// `/here` — back at the machine, and the ordinary rules apply again.
    Here,
    /// Words with no slash in front of them: something for a session to answer.
    Prompt(String),
    /// A slash word this build has no reading for, without its slash.
    Unknown(String),
    /// Nothing but space. Some clients send one; nothing is asked.
    Nothing,
}

/// Read one incoming line.
///
/// A command is the first word and nothing else, matched without regard to
/// case: a phone keyboard capitalizes the start of a message, so a rule that
/// only accepted `/sessions` would reject `/Sessions` for a reason invisible to
/// whoever typed it.
///
/// **Anything not starting with a slash is a prompt, whole.** No attempt is made
/// to find a command inside a sentence: a message beginning "help me work out
/// why…" is a prompt, and a parser clever enough to see a command in it would be
/// a parser that eats one message in fifty and never says which.
///
/// **A doubled slash is the way past that.** The agents have slash commands of
/// their own — a session offers `/compact`, `/clear` and whatever else it
/// advertises — and every one of them collides with this little language, so
/// without an escape they are simply unreachable from outside the app. `//x`
/// sends `/x` on to the agent. A doubled prefix rather than a `/send` wrapper
/// because the thing being typed is still a command and should still look like
/// one, and because an unknown word is a mistake worth reporting either way:
/// this build refuses `/compact` by name rather than guessing that anything it
/// does not recognise was meant for the agent, which would turn every mistyped
/// bridge command into a prompt nobody meant to send.
pub fn parse(line: &str) -> RemoteCommand {
    let line = line.trim();
    if line.is_empty() {
        return RemoteCommand::Nothing;
    }
    if let Some(escaped) = line.strip_prefix("//") {
        let escaped = escaped.trim();
        return if escaped.is_empty() {
            RemoteCommand::Nothing
        } else {
            RemoteCommand::Prompt(format!("/{escaped}"))
        };
    }
    let Some(rest) = line.strip_prefix('/') else {
        return RemoteCommand::Prompt(line.to_string());
    };

    let mut words = rest.split_whitespace();
    // A command sent to a bot in a group arrives addressed -- `/sessions@name`
    // -- because that is how the far side lets several bots share one room. The
    // address is about routing and is not part of the word.
    let word = words
        .next()
        .unwrap_or_default()
        .split('@')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    match word.as_str() {
        "sessions" => RemoteCommand::List,
        "away" => RemoteCommand::Away,
        "here" => RemoteCommand::Here,
        // A number that is not one, and a number that is missing, are the same
        // mistake to whoever made it: they meant to point this chat somewhere
        // and did not say where.
        "use" => match words.next().and_then(|n| n.parse::<u64>().ok()) {
            Some(uid) => RemoteCommand::Use(uid),
            None => RemoteCommand::UseWhich,
        },
        "archive" => RemoteCommand::Archive,
        // Counted from one, because that is how the listing is printed. Zero is
        // not a row anybody was offered, so it is the same mistake as a word.
        "open" => match words.next().and_then(|n| n.parse::<usize>().ok()) {
            Some(place) if place >= 1 => RemoteCommand::Open(place),
            _ => RemoteCommand::OpenWhich,
        },
        // `start` is what a client sends by itself the first time a chat is
        // opened, and it means "what is this". That is the same question `help`
        // asks, and answering it with "I do not know that word" is the worst
        // possible first sentence.
        "help" | "start" => RemoteCommand::Help,
        _ => RemoteCommand::Unknown(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_commands_this_build_knows() {
        assert_eq!(parse("/sessions"), RemoteCommand::List);
        assert_eq!(parse("/help"), RemoteCommand::Help);
        // The very first thing a fresh chat sends.
        assert_eq!(parse("/start"), RemoteCommand::Help);
        assert_eq!(parse("/use 7"), RemoteCommand::Use(7));
        assert_eq!(parse("/away"), RemoteCommand::Away);
        assert_eq!(parse("/here"), RemoteCommand::Here);
        assert_eq!(parse("/archive"), RemoteCommand::Archive);
        assert_eq!(parse("/open 2"), RemoteCommand::Open(2));
    }

    /// `/open` counts into the listing as printed, which starts at one. A zero
    /// is a row nobody was offered, so it is the same mistake as a word — and
    /// reading it as a place would take the first row on every mistyped press.
    #[test]
    fn open_needs_a_place_in_the_listing() {
        assert_eq!(parse("/open"), RemoteCommand::OpenWhich);
        assert_eq!(parse("/open 0"), RemoteCommand::OpenWhich);
        assert_eq!(parse("/open -1"), RemoteCommand::OpenWhich);
        assert_eq!(parse("/open the second"), RemoteCommand::OpenWhich);
        assert_eq!(parse("/open 1"), RemoteCommand::Open(1));
    }

    /// The two that say where the user is are opposites, and reading one as the
    /// other would silence the channel at exactly the moment it is wanted.
    #[test]
    fn away_and_here_are_not_confused_for_each_other() {
        assert_ne!(parse("/away"), parse("/here"));
        assert_eq!(parse("/AWAY@onehand_bot"), RemoteCommand::Away);
        // Not a command in the middle of a sentence, same as everything else.
        assert_eq!(
            parse("I'm away for an hour"),
            RemoteCommand::Prompt("I'm away for an hour".into())
        );
    }

    /// `/use` says where to point this chat, so a missing or unreadable number
    /// is one mistake with one answer rather than an unknown command.
    #[test]
    fn use_without_a_number_asks_which() {
        assert_eq!(parse("/use"), RemoteCommand::UseWhich);
        assert_eq!(parse("/use    "), RemoteCommand::UseWhich);
        assert_eq!(parse("/use the second one"), RemoteCommand::UseWhich);
        assert_eq!(parse("/use -1"), RemoteCommand::UseWhich);
        // Everything after the number is ignored rather than refused: a phone
        // adds a trailing space and a person adds a word.
        assert_eq!(parse("/use 7 please"), RemoteCommand::Use(7));
        assert_eq!(parse("/use@onehand_bot 7"), RemoteCommand::Use(7));
    }

    /// A phone keyboard capitalizes the first letter of a message, and
    /// surrounding space is not something anybody typed on purpose.
    #[test]
    fn case_and_surrounding_space_are_not_part_of_a_command() {
        assert_eq!(parse("  /Sessions  "), RemoteCommand::List);
        assert_eq!(parse("/HELP"), RemoteCommand::Help);
    }

    /// In a group the far side addresses a command to one bot by name, which is
    /// routing rather than part of the word.
    #[test]
    fn a_command_addressed_to_the_bot_is_still_that_command() {
        assert_eq!(parse("/sessions@onehand_bot"), RemoteCommand::List);
    }

    #[test]
    fn an_unknown_slash_word_is_named_so_the_reply_can_quote_it() {
        assert_eq!(parse("/deploy"), RemoteCommand::Unknown("deploy".into()));
        // A lone slash is a word of nothing, which is still not a prompt: the
        // user meant to type a command.
        assert_eq!(parse("/"), RemoteCommand::Unknown(String::new()));
    }

    /// The rule that keeps ordinary messages from being eaten: a command is the
    /// first word or it is not a command at all.
    #[test]
    fn a_sentence_that_merely_contains_a_command_is_a_prompt() {
        assert_eq!(
            parse("help me work out why the build is slow"),
            RemoteCommand::Prompt("help me work out why the build is slow".into())
        );
        assert_eq!(
            parse("run /sessions for me"),
            RemoteCommand::Prompt("run /sessions for me".into())
        );
    }

    /// A prompt keeps its own shape -- newlines and inner spacing are the
    /// message, and only what surrounds it is trimmed.
    #[test]
    fn a_prompt_arrives_whole() {
        assert_eq!(
            parse("\n fix the test\n\n  in editor.rs \n"),
            RemoteCommand::Prompt("fix the test\n\n  in editor.rs".into())
        );
    }

    /// The agents have slash commands of their own, and every one of them
    /// collides with this language. Without the escape they are unreachable
    /// from outside the app entirely.
    #[test]
    fn a_doubled_slash_sends_a_slash_command_on_to_the_agent() {
        assert_eq!(
            parse("//compact"),
            RemoteCommand::Prompt("/compact".into()),
            "the agent's own command, not this build's"
        );
        assert_eq!(
            parse("//model opus"),
            RemoteCommand::Prompt("/model opus".into())
        );
        // Even one this build *does* know: the escape is what says who it is
        // for, so `//sessions` is a prompt and not a listing.
        assert_eq!(
            parse("//sessions"),
            RemoteCommand::Prompt("/sessions".into())
        );
        assert_eq!(
            parse("  //  clear  "),
            RemoteCommand::Prompt("/clear".into())
        );
        // Nothing but slashes is nothing to send on.
        assert_eq!(parse("//"), RemoteCommand::Nothing);
    }

    /// An unknown word stays a mistake to report rather than being passed
    /// through. Guessing that anything unrecognised was meant for the agent
    /// would turn every mistyped bridge command into a prompt nobody sent.
    #[test]
    fn a_single_slash_is_never_forwarded() {
        assert_eq!(parse("/compact"), RemoteCommand::Unknown("compact".into()));
    }

    #[test]
    fn an_empty_line_asks_nothing() {
        assert_eq!(parse(""), RemoteCommand::Nothing);
        assert_eq!(parse("   \n\t "), RemoteCommand::Nothing);
    }
}
