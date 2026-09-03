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
    /// `/options` — what the bound session's agent lets you change, and to what.
    ///
    /// Mode, model, effort — the pickers the composer draws above the input, and
    /// the last thing about a session that could only be reached at the keyboard.
    Options,
    /// `/stop` — cancel the turn running on the session this chat is pointed at.
    ///
    /// The other half of being able to start one. Everything else here sets work
    /// going — a prompt, a granted permission, a reopened conversation — and an
    /// agent heading the wrong way is exactly the thing somebody who is not at
    /// the machine can do least about.
    Stop,
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
    /// `/follow` or `/follow <n>` — start hearing about this session.
    ///
    /// **The channel says nothing about a session nobody asked to hear about.**
    /// The opposite arrangement — everything speaks, and the noisy ones are
    /// silenced one at a time — makes the chat's contents a consequence of what
    /// happens to be open at the far end, which is a thing the reader neither
    /// chose nor can see. A machine running eight agents would have to be told
    /// about seven of them before it was quiet, and every session opened after
    /// that starts the argument again.
    ///
    /// The companion to [`Self::Away`] and its opposite in reach. Away is about
    /// the *user* and moves every followed session at once; this is about one
    /// conversation and says nothing about where anybody is.
    Follow(Aim),
    /// `/unfollow` or `/unfollow <n>` — go back to hearing nothing about it.
    Unfollow(Aim),
    /// `/status` — what will reach this chat, and what will not.
    ///
    /// **Not [`Self::List`] with different words.** That one answers "what is
    /// onehand running", which is about the app; this answers "what am I going
    /// to hear about", which is about the channel — and the two diverge on every
    /// fact that decides the second: whether the user has said they are away,
    /// which session this chat is pointed at, which sessions it follows. None of
    /// those is a property of a session, so none of them has a column in a
    /// listing of sessions.
    ///
    /// Worth a command of its own, and more so now that silence is the default.
    /// Every fact that decides whether anything arrives is invisible from the far
    /// side by construction: following nothing shows itself as messages that do
    /// not come, and so does being at the keyboard, and so does a bot whose
    /// process died an hour ago. This is the question that tells them apart, and
    /// it is the first thing to reach for when the channel seems broken.
    Status,
    /// Words with no slash in front of them: something for a session to answer.
    Prompt(String),
    /// A slash word this build has no reading for, without its slash.
    Unknown(String),
    /// Nothing but space. Some clients send one; nothing is asked.
    Nothing,
}

/// Which session a command that can take a number is about.
///
/// Its own type rather than an `Option<u64>` because there are three answers and
/// not two: a number, no number at all, and a word where a number was meant. The
/// last has to stay distinguishable from the middle one — falling back to "the
/// session this chat is pointed at" would quietly silence a conversation nobody
/// named, and the whole point of a mute is that it is invisible afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    /// No number: whatever this chat is already pointed at.
    Bound,
    /// A session named outright, so a chat can act on one it is not pointed at.
    Session(u64),
    /// A word where a number was expected.
    Unreadable,
}

/// Read the word after a command that takes an optional session number.
fn aim(word: Option<&str>) -> Aim {
    match word {
        None => Aim::Bound,
        Some(word) => match word.parse::<u64>() {
            Ok(uid) => Aim::Session(uid),
            Err(_) => Aim::Unreadable,
        },
    }
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
        // The number is optional here and required by `use`, and the difference
        // is what each one is for: `use` exists to say where, so a missing
        // number leaves it with nothing at all, while these two have an obvious
        // subject already — the session this chat is reading about.
        "follow" => RemoteCommand::Follow(aim(words.next())),
        "unfollow" => RemoteCommand::Unfollow(aim(words.next())),
        // `status` and not `watching`, though that is the question being asked.
        // The app already uses that word for the user's eyes being on a
        // conversation, which is what decides whether a turn is announced at
        // all — one word answering two questions in one feature is how the two
        // answers end up being given to the wrong one.
        "status" => RemoteCommand::Status,
        // A number that is not one, and a number that is missing, are the same
        // mistake to whoever made it: they meant to point this chat somewhere
        // and did not say where.
        "use" => match words.next().and_then(|n| n.parse::<u64>().ok()) {
            Some(uid) => RemoteCommand::Use(uid),
            None => RemoteCommand::UseWhich,
        },
        "archive" => RemoteCommand::Archive,
        "stop" => RemoteCommand::Stop,
        // `options` and not `settings`: these belong to the agent and change
        // with it, while the app's own settings are a dialog in a window and
        // nothing here can reach them.
        "options" => RemoteCommand::Options,
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
        assert_eq!(parse("/stop"), RemoteCommand::Stop);
        assert_eq!(parse("/options"), RemoteCommand::Options);
        assert_eq!(parse("/follow"), RemoteCommand::Follow(Aim::Bound));
        assert_eq!(parse("/unfollow"), RemoteCommand::Unfollow(Aim::Bound));
        assert_eq!(parse("/status"), RemoteCommand::Status);
    }

    /// The channel used to speak about everything and be silenced one session at
    /// a time. It no longer does, and the old words must not still work: a
    /// `/mute` that quietly did nothing would leave somebody believing a session
    /// had been silenced, which is the one mistake whose symptom is more
    /// messages rather than fewer.
    #[test]
    fn the_silencing_this_replaced_is_gone_rather_than_aliased() {
        assert_eq!(parse("/mute"), RemoteCommand::Unknown("mute".into()));
        assert_eq!(parse("/unmute"), RemoteCommand::Unknown("unmute".into()));
    }

    /// `/status` asks about the channel and `/sessions` about the app, so they
    /// must not collapse into each other — and a sentence about status is a
    /// prompt like any other sentence.
    #[test]
    fn status_is_its_own_question() {
        assert_ne!(parse("/status"), parse("/sessions"));
        assert_eq!(parse("/STATUS@onehand_bot"), RemoteCommand::Status);
        // It takes no argument, so trailing words are the phone's, not a target.
        assert_eq!(parse("/status of the build"), RemoteCommand::Status);
        assert_eq!(
            parse("status on the migration please"),
            RemoteCommand::Prompt("status on the migration please".into())
        );
    }

    /// The number is optional, and a session can be followed without this chat
    /// being pointed at it — what you want telling about is not always what you
    /// are typing into.
    #[test]
    fn follow_takes_a_session_or_the_one_this_chat_is_on() {
        assert_eq!(parse("/follow 7"), RemoteCommand::Follow(Aim::Session(7)));
        assert_eq!(
            parse("/unfollow 7"),
            RemoteCommand::Unfollow(Aim::Session(7))
        );
        assert_eq!(
            parse("/FOLLOW@onehand_bot 7"),
            RemoteCommand::Follow(Aim::Session(7))
        );
        // Trailing words are the phone's, not the user's, same as `/use`.
        assert_eq!(
            parse("/follow 7 please"),
            RemoteCommand::Follow(Aim::Session(7))
        );
    }

    /// A word where a number was meant is not "the one this chat is on". On
    /// `/unfollow` reading it that way would silence a conversation nobody
    /// named, and that is the mistake you do not notice: what it costs is every
    /// message the session would have sent, up until somebody wonders why it
    /// went quiet.
    #[test]
    fn a_word_where_a_session_number_was_meant_is_not_a_default() {
        assert_eq!(
            parse("/follow this one"),
            RemoteCommand::Follow(Aim::Unreadable)
        );
        assert_eq!(parse("/follow all"), RemoteCommand::Follow(Aim::Unreadable));
        assert_eq!(
            parse("/unfollow -1"),
            RemoteCommand::Unfollow(Aim::Unreadable)
        );
        assert_ne!(parse("/unfollow everything"), parse("/unfollow"));
    }

    /// `/follow` and `/unfollow` are opposites and one is the other's prefix,
    /// which is exactly the pair a sloppy first-word match collapses.
    #[test]
    fn follow_and_unfollow_are_not_confused_for_each_other() {
        assert_ne!(parse("/follow"), parse("/unfollow"));
        assert_eq!(
            parse("follow the stack trace up"),
            RemoteCommand::Prompt("follow the stack trace up".into())
        );
    }

    /// `/stop` is the one command whose misreading costs work rather than a
    /// puzzled reply, so it is worth pinning that it is not reachable by
    /// accident — a sentence about stopping is a prompt like any other.
    #[test]
    fn stop_is_a_command_and_a_sentence_about_it_is_not() {
        assert_eq!(parse("/STOP@onehand_bot"), RemoteCommand::Stop);
        assert_eq!(
            parse("stop when the tests pass"),
            RemoteCommand::Prompt("stop when the tests pass".into())
        );
        // And the escape sends the agent's own, if it has one.
        assert_eq!(parse("//stop"), RemoteCommand::Prompt("/stop".into()));
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
