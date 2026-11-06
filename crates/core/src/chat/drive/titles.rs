//! A conversation's name: the one a person gives it, written down as soon as
//! it changes, and put back when that write fails.
//!
//! **Shown at once, written after, in order.** A rename is often the last
//! thing done to a finished conversation, so it is written then rather than
//! at the next turn's end; and the name alone, not the transcript, since a
//! rename can land mid-turn. Each write goes after every name written before
//! it, and a failure puts back the last name known written, said once.

use super::{Effect, SessionDrive, Signal};
use crate::saving::Saving;

impl SessionDrive {
    /// Name the conversation `title`. Whether anything changed: a blank name
    /// is not a rename.
    pub fn rename(&mut self, title: &str) -> (bool, Vec<Effect>) {
        let before = self.chat.custom_title.clone();
        if !self.chat.rename(title) {
            return (false, Vec::new());
        }
        (true, self.save_title(before))
    }

    /// Drop the name a person gave, back to the automatic one: a worker's
    /// folder, else the one taken from the first prompt.
    pub fn reset_title(&mut self) -> Vec<Effect> {
        let before = self.chat.custom_title.clone();
        self.chat.reset_title();
        self.save_title(before)
    }

    /// Carry the names of the session this one takes over from: its given
    /// name, unless this one has one already, and what stands in for it.
    pub fn carry_titles(&mut self, custom: Option<String>, stand_in: Option<String>) {
        self.chat.carry_titles(custom, stand_in);
    }

    /// Name a session opened on a worker's folder for that folder, with the
    /// name it was given over it. Nothing is written: no conversation file
    /// exists yet, and the first turn's end writes the name with the rest.
    pub fn name_for_folder(&mut self, folder: String, given: Option<String>) {
        self.chat.stand_in_title = Some(folder);
        self.chat.custom_title = given;
    }

    fn save_title(&mut self, before: Option<String>) -> Vec<Effect> {
        let title = self.chat.custom_title.clone();
        let change = self
            .title
            .get_or_insert_with(|| Saving::new(before))
            .change(title.clone());
        self.chat
            .flush_meta()
            .map(|write| Effect::SaveTitle {
                write: Box::new(write),
                change,
                title,
            })
            .into_iter()
            .collect()
    }

    pub(super) fn title_saved(
        &mut self,
        change: u64,
        title: Option<String>,
        result: Result<(), String>,
    ) -> Vec<Effect> {
        let Some(saving) = self.title.as_mut() else {
            return Vec::new();
        };
        let Some(why) = saving.landed(change, title, result) else {
            return Vec::new();
        };
        match saving.shown().clone() {
            Some(name) => {
                self.chat.rename(&name);
            }
            None => self.chat.reset_title(),
        }
        vec![Effect::Signal(Signal::RenameFailed(why))]
    }
}
