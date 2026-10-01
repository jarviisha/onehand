use super::items::ChatItem;
use super::{Chat, Replay};
use crate::chat::store;
use crate::chat::store::{ConversationSnapshot, MetaWrite, PendingWrite};

impl Chat {
    /// Whether an adopted archive is still waiting for the agent to replay it.
    ///
    /// Only the tests ask this: the app never inspects the replay window, it
    /// just keeps prompting. [`Chat::flush`] is what consults the state before
    /// anything is written, and [`Chat::take_snapshot`] is what settles it.
    #[cfg(test)]
    pub(super) fn replay_pending(&self) -> bool {
        matches!(self.replay, Replay::Armed)
    }

    /// Everything not yet on disk, and the metadata that goes with it.
    ///
    /// `None` when there is nowhere to write, nothing to identify the
    /// conversation by, or nothing new — which is what keeps a session nobody
    /// used from creating a directory and standing in the list beside
    /// conversations that were had.
    ///
    /// The mark moves here, as the save is *built* rather than when it lands,
    /// so a second save started before the first has finished finds nothing
    /// left to add instead of adding it twice.
    pub fn flush(&mut self) -> Option<PendingWrite> {
        // Nothing is written while a replay is in flight. What `items` holds
        // then is a re-delivery of lines that are already on disk, at indices
        // that mean a different thing on each side of the comparison.
        if matches!(self.replay, Replay::Partial(_)) {
            return None;
        }
        let total = self.history.len() + self.items.len();
        let from = if self.rewrite { 0 } else { self.persisted };
        if from >= total {
            return None;
        }

        let mut lines = Vec::new();
        let mut blobs = Vec::new();
        for item in self.history.iter().chain(self.items.iter()).skip(from) {
            if let Some((line, mut carried)) = store::line_of(self, item) {
                lines.push(line);
                blobs.append(&mut carried);
            }
        }

        let mut write = self.meta_write(total)?;
        // Only a save that added messages moves the conversation's date -- see
        // the field on the file for why the list must not reorder otherwise.
        write.meta.updated = (!lines.is_empty()).then(store::now_secs);
        write.lines = lines;
        write.blobs = blobs;
        write.rewrite = self.rewrite;

        self.persisted = total;
        self.rewrite = false;
        Some(write)
    }

    /// The metadata alone — a rename, a reset, a change of selector.
    ///
    /// Separate from [`Self::flush`] because these can happen *during* a turn,
    /// and a line written mid-turn describes a tool call that has not finished.
    /// It would never be revisited: the turn's own save writes only what came
    /// after it, so the half-finished card is what the conversation would hold
    /// from then on.
    ///
    /// `None` for a conversation with nothing on disk yet. Metadata by itself
    /// would put an empty conversation in the list.
    pub fn flush_meta(&mut self) -> Option<PendingWrite> {
        if self.persisted == 0 {
            return None;
        }
        self.meta_write(self.persisted)
    }

    /// The metadata half of a save, with no lines in it yet.
    fn meta_write(&self, items: usize) -> Option<PendingWrite> {
        let store = self.store.as_ref()?;
        let session_id = self.session_id.clone()?;
        Some(PendingWrite {
            dir: store::conv_dir(store, &session_id),
            lines: Vec::new(),
            blobs: Vec::new(),
            rewrite: false,
            meta: MetaWrite {
                session_id,
                root: self.root.display().to_string(),
                agent: self.agent.clone(),
                title: self.custom_title.clone(),
                preview: self.first_prompt(),
                prefs: self.prefs(),
                updated: None,
                items,
            },
        })
    }

    /// Build and write, for the paths with no async context to hand the write
    /// off to — a session being taken apart, or the app closing.
    ///
    /// A failure is logged and nothing more, deliberately: this runs while the
    /// window it would have spoken to is being torn down. The per-turn save
    /// covers the same conversation every turn and does report, so a standing
    /// condition has already been said out loud long before this runs.
    pub fn save_blocking(&mut self) {
        if let Some(write) = self.flush() {
            if let Err(e) = store::commit(&write) {
                eprintln!("onehand: conversation not archived: {e}");
            }
        }
    }

    /// Lift this conversation out, leaving the chat empty — the handoff a
    /// restart makes to the session replacing it.
    ///
    /// A move rather than a copy, and that is what makes it safe: the chat it
    /// came from is about to be dropped, and a drop still holding these items
    /// would write them a second time. It carries the mark with it, so the
    /// replacement continues the same file rather than starting one.
    pub fn take_snapshot(&mut self) -> Option<ConversationSnapshot> {
        let session_id = self.session_id.clone()?;
        // Whatever is known to be whole. Mid-replay that is the copy that was
        // adopted, not the re-delivery that has not finished arriving.
        let items = match std::mem::replace(&mut self.replay, Replay::Settled) {
            Replay::Partial(stash) => {
                self.history.clear();
                self.items.clear();
                stash
            }
            Replay::Settled | Replay::Armed => {
                let mut items = std::mem::take(&mut self.history);
                items.append(&mut self.items);
                items
            }
        };
        // The cards that are a question go, since the adapter that asked is the
        // one being replaced and an answer would reach nobody. Dropping one
        // shifts every position after it, so any dropped from before the mark
        // come off the mark too.
        let mut dropped_before = 0;
        let mut kept = Vec::with_capacity(items.len());
        for (at, item) in items.into_iter().enumerate() {
            if matches!(item, ChatItem::Permission(_) | ChatItem::Ask(_)) {
                if at < self.persisted {
                    dropped_before += 1;
                }
                continue;
            }
            kept.push(item);
        }

        Some(ConversationSnapshot {
            session_id,
            title: self.custom_title.clone(),
            updated: self.last_activity.unwrap_or_else(store::now_secs),
            // The file's own, and the file keeps it: a snapshot passing through
            // memory has no business telling a conversation when it began.
            prefs: self.prefs(),
            items: kept,
            written: self.persisted.saturating_sub(dropped_before),
            complete: !self.bounded,
        })
    }

    /// Close the replay window, putting the adopted copy back if the replay
    /// came up short of it.
    ///
    /// Short is the only case that restores. A replay that delivered at least as
    /// much *is* the better record — it is the agent's own, and it can be longer
    /// than the file when a previous run died between the last save and the end
    /// of a turn. Locally-minted notices are not part of the count and are kept
    /// either way: they say why the resume went as it did, which is exactly what
    /// the reader needs on screen when this puts a conversation back.
    ///
    /// The two outcomes leave the file in different states, and that is why the
    /// rewrite exists. Putting the copy back leaves the file as it was, so the
    /// mark is what it was. Keeping the replay means the file has to *become*
    /// it: a re-delivery is chunked as the agent chose rather than as the file
    /// was, so adding to it at an index that means a different thing on each
    /// side is how a seam drops or doubles a message. Once per resume, and the
    /// same size of read the resume already paid for. Never when the transcript
    /// came back bounded, because then what is on screen is a tail and the file
    /// is longer than it.
    pub(super) fn settle_replay(&mut self) {
        let Replay::Partial(stash) = std::mem::replace(&mut self.replay, Replay::Settled) else {
            return;
        };
        let replayed = self
            .items
            .iter()
            .filter(|it| !matches!(it, ChatItem::Notice { .. }))
            .count();
        if replayed < stash.len() || self.bounded {
            self.persisted = stash.len();
            self.history = stash;
            self.items
                .retain(|it| matches!(it, ChatItem::Notice { .. }));
            self.touch();
        } else {
            self.rewrite = true;
            self.persisted = 0;
        }
    }

    /// Load a resumed conversation's transcript as read-only history.
    ///
    /// Arms `replay_pending` immediately: a `session/load` resume replays its
    /// history as `session/update`s, and those can arrive *before* the
    /// `Connected { resumed: true }` event (the load response). Arming here — not
    /// on `Connected` — means the first replayed content drops the placeholder
    /// history regardless of event order; a `session/new` fallback disarms it via
    /// `Connected { resumed: false }` so old history stays as read-only context.
    /// `written` is how many of `items` are already on disk — the mark the
    /// conversation carries on from.
    pub(crate) fn load_history(
        &mut self,
        items: Vec<ChatItem>,
        session_id: String,
        written: usize,
    ) {
        // The loaded transcript *is* this conversation, and a `session/load`
        // replay re-delivers it into `items`. Without the reset the view
        // (history ⧺ items) shows everything twice.
        //
        // Nothing is saved on the way past any more. Under a file that is only
        // added to there is nothing to rescue: whatever the live items held is
        // either already written, or is being carried in here with the mark
        // that says so.
        self.items.clear();
        self.terminals.clear();
        self.history = items;
        self.persisted = written;
        self.session_id = Some(session_id);
        self.replay = Replay::Armed;
        self.rewrite = false;
        self.touch();
    }

    /// Adopt a conversation: its transcript, its title, the selector state to
    /// replay, when it was last touched, and how much of it is on disk.
    ///
    /// They happen together and none of them is optional, which is why this is
    /// one call rather than five at a call site. Forget `arm_prefs` and a
    /// reopened conversation silently loses its effort/agent (the adapter
    /// rebuilds those from static settings on `session/load`); forget
    /// `last_activity` and the rail dates the session from the moment it was
    /// reopened rather than from its last real turn; forget the mark and the
    /// conversation is written to its own file a second time.
    pub fn resume_from(&mut self, snapshot: ConversationSnapshot) {
        self.bounded = !snapshot.complete;
        self.custom_title = snapshot.title;
        self.last_activity = Some(snapshot.updated);
        self.arm_prefs(
            snapshot.prefs.mode,
            snapshot
                .prefs
                .config
                .into_iter()
                .map(|c| (c.id, c.value))
                .collect(),
        );
        self.load_history(snapshot.items, snapshot.session_id, snapshot.written);
    }

    /// Take the loaded history off the screen the first time real replayed
    /// content arrives — and keep hold of it, because "the replay has begun" is
    /// not "the replay has finished". See [`Replay`].
    pub(super) fn consume_replay(&mut self) {
        if matches!(self.replay, Replay::Armed) {
            self.replay = Replay::Partial(std::mem::take(&mut self.history));
        }
    }
}

/// Archive the conversation when the chat is dropped (session closed / app
/// exit). `save_chat` is a no-op while empty, so this never clobbers.
///
/// A failure here is logged and nothing more, which is the whole answer at this
/// one point: a chat is dropped while its window is being torn down, so there
/// is nothing left to raise a message on, and blocking teardown on one would be
/// worse than the loss. The per-turn save covers the same conversation every
/// turn and *does* speak up, so any standing condition — a full disk, a
/// read-only directory — has already been said out loud long before this runs.
impl Drop for Chat {
    fn drop(&mut self) {
        self.save_blocking();
    }
}
