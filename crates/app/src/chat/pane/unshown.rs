//! Sessions started off screen: an unattended run's, a tool's worker, and a
//! lead's worker coming back, which waits unconnected until it is shown or
//! its lead calls on it.

use super::ChatPane;
use crate::chat::conversation::Conversation;
use crate::chat::session::ChatSession;
use gpui::{Context, Entity};
use onehand_core::config::AgentSpec;
use onehand_core::workspace::drive::StartsOn;
use std::collections::hash_map::Entry;
use std::path::PathBuf;

impl ChatPane {
    /// Start `uid` on `root` and connect it, without putting it on screen.
    ///
    /// The other half of the lazy rule [`Self::show`] keeps. A session normally
    /// connects the first time it is shown, so that a workspace of a dozen roots
    /// does not launch a dozen agents at boot; an unattended run is one agent
    /// that was asked for, and showing it would swap the conversation somebody
    /// is reading for one they did not start. Nothing here needs a window — it
    /// is showing, not connecting, that does.
    pub fn open_unshown(
        &mut self,
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatSession>> {
        if let Entry::Vacant(slot) = self.conversations.entry(uid) {
            slot.insert(Conversation {
                dirs,
                ..Conversation::opening(root, spec.clone())
            });
            self.connect(uid, None, cx);
        }
        self.session_of(uid).cloned()
    }

    /// Session `uid`, added and not connected, may also write `dirs` once it
    /// connects.
    pub fn hold_dirs(&mut self, uid: u64, dirs: Vec<PathBuf>) {
        self.held_dirs.insert(uid, dirs);
    }

    /// Connect `uid` on `root` without putting it on screen, starting on
    /// `starts`; nothing when it has begun connecting already.
    pub fn connect_unshown(
        &mut self,
        uid: u64,
        root: PathBuf,
        spec: &AgentSpec,
        starts: StartsOn,
        cx: &mut Context<Self>,
    ) {
        let Entry::Vacant(slot) = self.conversations.entry(uid) else {
            return;
        };
        slot.insert(Conversation {
            dirs: self.held_dirs.remove(&uid).unwrap_or_default(),
            ..Conversation::opening(root, spec.clone())
        });
        match starts {
            StartsOn::Resume(archive) => self.resume(uid, archive, cx),
            // Only a conversation to resume is ever added unconnected.
            StartsOn::Fresh | StartsOn::Fork | StartsOn::Watch(_) => self.connect(uid, None, cx),
        }
    }
}
