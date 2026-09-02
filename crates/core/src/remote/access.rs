//! Who is allowed to reach the app from outside it.
//!
//! One rule, in one place, because it is the whole security model of the bridge:
//! a bot token identifies the *bot*, not the person typing at it, so anybody who
//! guesses the bot's name can open a chat with it. The list is what turns that
//! into a private channel.

use super::types::ChatId;

/// Whether `chat` may reach the app.
///
/// **The empty list allows nobody.** That is the useful reading and not a
/// degenerate one: an enabled bridge with no list is a bot that anyone who finds
/// it can drive, so forgetting to fill the list in has to fail closed.
///
/// Compared as trimmed text rather than as numbers, because that is what the
/// model carries end to end and what the next channel will hand over — and
/// because a hand-typed list has spaces in it.
pub fn is_allowed(allowed: &[String], chat: &ChatId) -> bool {
    let chat = chat.trim();
    !chat.is_empty() && allowed.iter().any(|allowed| allowed.trim() == chat)
}

/// Whether an incoming message deserves any reply at all.
///
/// **A message from a chat that is not on the list is answered with nothing** —
/// not an error, not a refusal, not a read receipt. A refusal is a confirmation:
/// it tells whoever sent it that the bot is real, that it is running right now,
/// and that there is a list to get onto. Silence tells them nothing, which is
/// the only answer that does not help.
///
/// Named as its own function rather than left as a `!is_allowed` at the call
/// site so the rule has somewhere to be written down, and so the one place that
/// could accidentally start replying has to say the word `silently`.
pub fn is_silently_ignored(allowed: &[String], chat: &ChatId) -> bool {
    !is_allowed(allowed, chat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_listed_chat_is_allowed() {
        let allowed = list(&["123", "-100456"]);
        assert!(is_allowed(&allowed, &"123".to_string()));
        assert!(is_allowed(&allowed, &"-100456".to_string()));
        assert!(!is_allowed(&allowed, &"124".to_string()));
    }

    /// The failure mode this exists to prevent: an enabled bridge whose list was
    /// never filled in must be reachable by nobody, not by everybody.
    #[test]
    fn an_empty_list_allows_nobody() {
        assert!(!is_allowed(&[], &"123".to_string()));
        assert!(is_silently_ignored(&[], &"123".to_string()));
    }

    /// A hand-typed list has spaces in it, and a chat id that only differs by
    /// one is the same chat.
    #[test]
    fn surrounding_space_is_not_part_of_an_id() {
        assert!(is_allowed(&list(&[" 123 "]), &"123".to_string()));
        assert!(is_allowed(&list(&["123"]), &" 123".to_string()));
    }

    /// An id that is nothing at all matches nothing, including a list entry that
    /// is also nothing — an empty line left in the config must not become a
    /// wildcard.
    #[test]
    fn an_empty_id_matches_nothing() {
        assert!(!is_allowed(&list(&["", "123"]), &"".to_string()));
        assert!(!is_allowed(&list(&["   "]), &"  ".to_string()));
    }

    #[test]
    fn anything_not_allowed_is_ignored_without_a_word() {
        let allowed = list(&["123"]);
        assert!(!is_silently_ignored(&allowed, &"123".to_string()));
        assert!(is_silently_ignored(&allowed, &"999".to_string()));
    }
}
