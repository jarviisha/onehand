//! What the reader has open: every fold in a session's transcript, and the
//! diffs a turn's file rows open into.
//!
//! **View state, kept here and nowhere in core.** What opens by itself (a
//! step running or one that just failed, a thought still being written) is
//! the model's to say; what the reader opened or closed on top of that is
//! this side's, keyed by position, and forgotten when the transcript is
//! numbered afresh.

use super::ChatSession;
use onehand_core::chat::TranscriptItemId;

impl ChatSession {
    /// How many times a fold was toggled, for a layout built from the folds.
    pub fn folds(&self) -> u64 {
        self.folds
    }

    fn fold_changed(&mut self) {
        self.folds = self.folds.wrapping_add(1);
    }

    /// Whether the item at `target` is open: the reader's choice once they
    /// made one, else `default`, what the item does by itself.
    pub fn item_is_open(&self, target: TranscriptItemId, default: bool) -> bool {
        self.item_open.get(&target).copied().unwrap_or(default)
    }

    /// Open or close the item at `target`, which shows `default` until the
    /// reader chooses.
    ///
    /// **The choice is kept against a default that moves**: a step opens
    /// while it runs and when it fails, and a reader who closed it keeps it
    /// closed, so a failure never forces a row open with no way to shut it.
    pub fn toggle_item(&mut self, target: TranscriptItemId, default: bool) {
        let now = self.item_is_open(target, default);
        self.item_open.insert(target, !now);
        self.fold_changed();
    }

    /// Whether the step at `target` shows output section `section` past its
    /// cut.
    pub fn out_is_open(&self, target: TranscriptItemId, section: usize) -> bool {
        self.out_open.contains(&(target, section))
    }

    /// Show output section `section` of the step at `target` past its cut, or
    /// cut it again. Leaves `folds` alone: the run layout never reads an
    /// output well, and the row it sits in is drawn from the item either way.
    pub fn toggle_out(&mut self, target: TranscriptItemId, section: usize) {
        if !self.out_open.remove(&(target, section)) {
            self.out_open.insert((target, section));
        }
    }

    /// Whether the prompt at `target` is shown whole past its cut.
    pub fn prompt_is_open(&self, target: TranscriptItemId) -> bool {
        self.prompt_open.contains(&target)
    }

    /// Show the prompt at `target` whole, or cut it again.
    pub fn toggle_prompt(&mut self, target: TranscriptItemId) {
        if !self.prompt_open.remove(&target) {
            self.prompt_open.insert(target);
        }
        self.fold_changed();
    }

    /// Whether the activity run anchored at `anchor` is showing its steps:
    /// the reader's choice once they made one, else `default`.
    pub fn activity_is_open(&self, anchor: TranscriptItemId, default: bool) -> bool {
        self.activity_open.get(&anchor).copied().unwrap_or(default)
    }

    /// Hold the activity run anchored at `anchor` open or closed, whatever
    /// it goes on to do.
    pub fn set_activity_open(&mut self, anchor: TranscriptItemId, open: bool) {
        self.activity_open.insert(anchor, open);
        self.fold_changed();
    }

    /// Whether the section anchored at `anchor` is showing its steps.
    pub fn section_is_open(&self, anchor: TranscriptItemId) -> bool {
        self.section_open.contains(&anchor)
    }

    /// Fold or unfold the section anchored at `anchor`.
    pub fn toggle_section(&mut self, anchor: TranscriptItemId) {
        if !self.section_open.remove(&anchor) {
            self.section_open.insert(anchor);
        }
        self.fold_changed();
    }

    /// Whether the turn summary anchored at `anchor` is open, given what it
    /// would do if nobody had said.
    pub fn turn_is_open(&self, anchor: TranscriptItemId, default: bool) -> bool {
        self.turn_open.get(&anchor).copied().unwrap_or(default)
    }

    /// Open or close the turn summary anchored at `anchor`.
    pub fn toggle_turn(&mut self, anchor: TranscriptItemId, default: bool) {
        let now = self.turn_is_open(anchor, default);
        self.turn_open.insert(anchor, !now);
        self.fold_changed();
    }

    /// Whether the subagent card `id` shows every step it took.
    pub fn subagent_is_open(&self, id: &str) -> bool {
        self.subagent_open.contains(id)
    }

    /// Open or close the subagent card `id`.
    pub fn toggle_subagent(&mut self, id: &str) {
        if !self.subagent_open.remove(id) {
            self.subagent_open.insert(id.to_string());
        }
        self.fold_changed();
    }

    /// Whether `path`'s diff is showing under the turn anchored at `anchor`.
    pub fn file_is_open(&self, anchor: TranscriptItemId, path: &str) -> bool {
        // **Built and asked, not scanned.** This is answered once per file row
        // per frame, so walking the set turns a turn's worth of rows into a
        // quadratic over what the whole conversation has open.
        self.file_open.contains_key(&(anchor, path.to_string()))
    }

    /// Show or hide `path`'s diff under the turn anchored at `anchor`.
    ///
    /// **The diff is computed here, once, and kept.** Taking it in the renderer
    /// instead put an LCS over two whole files inside a render pass -- which
    /// runs on every frame the row is on screen, for as long as it is open, and
    /// the table that diff builds is megabytes wide on a large file. The row
    /// changes what it draws only when somebody clicks it, so this is the one
    /// moment the work is actually needed.
    pub fn toggle_file(&mut self, anchor: TranscriptItemId, path: &str, body: &[TranscriptItemId]) {
        let key = (anchor, path.to_string());
        if self.file_open.remove(&key).is_none() {
            let rows = self.with_turn_items(body, |items| {
                onehand_core::chat::turn_file_diff(items, path)
            });
            self.file_open.insert(key, rows);
        }
        self.fold_changed();
    }

    /// The diff showing under `path`'s row, if it is showing.
    pub fn file_diff(
        &self,
        anchor: TranscriptItemId,
        path: &str,
    ) -> Option<&[onehand_core::diff::Row]> {
        self.file_open
            .get(&(anchor, path.to_string()))
            .map(Vec::as_slice)
    }

    /// Show every file of the turn anchored at `anchor`, or hide them all if
    /// they are already showing.
    ///
    /// One control for the whole block, since opening eight rows one at a time
    /// to read a turn's diff is the block asking to be scrolled rather than
    /// read.
    pub fn toggle_every_file(
        &mut self,
        anchor: TranscriptItemId,
        paths: &[String],
        body: &[TranscriptItemId],
    ) {
        let all_open = paths.iter().all(|path| self.file_is_open(anchor, path));
        self.file_open.retain(|(at, _), _| *at != anchor);
        if !all_open {
            let rows: Vec<(String, Vec<onehand_core::diff::Row>)> =
                self.with_turn_items(body, |items| {
                    paths
                        .iter()
                        .map(|path| {
                            (
                                path.clone(),
                                onehand_core::chat::turn_file_diff(items, path),
                            )
                        })
                        .collect()
                });
            self.file_open
                .extend(rows.into_iter().map(|(path, diff)| ((anchor, path), diff)));
        }
        self.fold_changed();
    }

    /// Run `f` over the transcript items `body` names.
    ///
    /// The summary block holds a turn's items by id rather than by value, so
    /// this is how it reaches them when a row is opened -- and it is a closure
    /// rather than a returned `Vec<&ChatItem>` because those borrow the chat
    /// this session owns.
    pub fn with_turn_items<T>(
        &self,
        body: &[TranscriptItemId],
        f: impl FnOnce(&[&onehand_core::chat::ChatItem]) -> T,
    ) -> T {
        let items: Vec<&onehand_core::chat::ChatItem> = body
            .iter()
            .filter_map(|target| match target {
                TranscriptItemId::History(i) => self.chat.history.get(*i),
                TranscriptItemId::Live(i) => self.chat.items.get(*i),
            })
            .collect();
        f(&items)
    }

    /// Folds are keyed by position, and the transcript is numbered afresh,
    /// so a choice kept from before would land on an unrelated step.
    pub(super) fn forget_folds(&mut self) {
        self.numbering = self.chat.numbering();
        self.activity_open.clear();
        self.section_open.clear();
        self.prompt_open.clear();
        self.turn_open.clear();
        self.file_open.clear();
        self.item_open.clear();
        self.out_open.clear();
        self.fold_changed();
    }
}
