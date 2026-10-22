//! Putting a conversation on disk, and taking it off again.
//!
//! Everything here runs under one lock per conversation directory, and that
//! lock is the reason it stands apart from the reading side: a save and a
//! delete are ordered against each other through it, and the lock also holds
//! the newest metadata mark written there, which is what lets a save read
//! earlier than one that already landed keep its hands off that one's fields.
//! Reading takes no lock and needs none of this.

use super::{now_secs, Meta, MetaWrite, PendingWrite, FORMAT};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Apply a prepared save: the new lines, their blobs, then the metadata.
///
/// Blocking; call it off the UI loop. Errors are returned rather than swallowed
/// — a transcript is the one thing here the user cannot produce again.
pub fn commit(write: &PendingWrite) -> std::io::Result<()> {
    // One writer per conversation within this process, so two windows or two
    // sessions on one conversation are ordered rather than interleaved. The
    // key is the conversation itself, so unlike a check on which window holds
    // what, it cannot fail to notice a second writer.
    let lock = writer_for(&write.dir);
    let mut newest_landed = lock.lock().unwrap_or_else(|e| e.into_inner());

    std::fs::create_dir_all(&write.dir)?;
    if !write.blobs.is_empty() {
        let blobs = write.dir.join("blobs");
        std::fs::create_dir_all(&blobs)?;
        for (name, bytes) in &write.blobs {
            // Content-addressed, so writing one that is already there is the
            // same file again — which is what makes a rewrite safe to repeat.
            let path = blobs.join(name);
            if !path.exists() {
                // Waited for, like the line that will point at it: a blob that
                // is half on disk is a picture that decodes to nothing, and the
                // card would have no way to know it is showing a ruin.
                crate::config::write_synced(&path, bytes.as_slice())?;
            }
        }
    }

    let items = write.dir.join("items.jsonl");
    if write.rewrite && !write.lines.is_empty() {
        // The one write here that can take something away, so it is also the
        // one with a condition on it: replacing a transcript with nothing is
        // never what a caller meant, and this is the only path that could.
        let mut body = write.lines.join("\n");
        body.push('\n');
        crate::config::write_atomic(&items, &body)?;
    } else if !write.rewrite && !write.lines.is_empty() {
        // Opened fresh and closed each time, and written in one call: appending
        // is then a single operation the kernel does not split, so two
        // processes adding to one conversation interleave whole turns rather
        // than halves of a line. Holding the file open would also leave a
        // writer adding into an inode a rewrite had already replaced.
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&items)?;
        // A crash mid-write leaves the last line without its newline. Added
        // straight onto it, this save's first line would join a fragment that
        // never parses, and be lost with it.
        let mut body = if ends_torn(&mut file)? {
            "\n".to_string()
        } else {
            String::new()
        };
        body.push_str(&write.lines.join("\n"));
        body.push('\n');
        file.write_all(body.as_bytes())?;
        // Waited for before this call answers. Without it "written at the end
        // of every turn" means the kernel has the turn, not the disk — and the
        // whole reason the write happens per turn rather than at the end of the
        // session is to survive the machine going away.
        file.sync_all()?;
    }

    // Metadata read before what already landed would put its older count,
    // preview and name back over the newer ones: only its date is kept.
    let stale = write.meta.read_at < *newest_landed;
    write_meta(&write.dir, &write.meta, stale)?;
    if !stale {
        *newest_landed = write.meta.read_at;
    }
    Ok(())
}

/// Whether `file` is non-empty and does not end in a newline.
fn ends_torn(file: &mut std::fs::File) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

/// Write `meta.json`, keeping the fields that are the file's to remember.
///
/// `created` and — when this save added no messages — `updated` are read back
/// off the existing file rather than carried in. They belong to the
/// conversation's history rather than to the moment being saved, and a rename
/// that reordered the picker would be the list rearranging itself for something
/// that is not new work.
///
/// `stale` is metadata read before metadata that has already landed: of it,
/// only a later date for added messages is written, over what is there.
fn write_meta(dir: &Path, meta: &MetaWrite, stale: bool) -> std::io::Result<()> {
    let path = dir.join("meta.json");
    let existing: Option<Meta> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let existing = match (stale, existing) {
        (true, Some(mut existing)) => {
            return match meta.updated {
                Some(at) if at > existing.updated => {
                    existing.updated = at;
                    let text =
                        serde_json::to_string_pretty(&existing).map_err(std::io::Error::other)?;
                    crate::config::write_atomic(&path, &text)
                }
                _ => Ok(()),
            };
        }
        // Nothing there to keep: the newer write is gone, so this is all
        // there is to say.
        (_, existing) => existing,
    };
    let now = now_secs();
    let out = Meta {
        format: FORMAT,
        session_id: meta.session_id.clone(),
        root: meta.root.clone(),
        agent: meta.agent.clone(),
        created: existing.as_ref().map(|m| m.created).unwrap_or(now),
        updated: meta
            .updated
            .or_else(|| existing.as_ref().map(|m| m.updated))
            .unwrap_or(now),
        items: meta.items,
        title: meta.title.clone(),
        preview: meta.preview.clone(),
        prefs: meta.prefs.clone(),
    };
    let text = serde_json::to_string_pretty(&out).map_err(std::io::Error::other)?;
    crate::config::write_atomic(&path, &text)
}

/// Remove a conversation and everything kept with it. Blocking.
///
/// The whole directory, which is what makes this one call rather than a hunt:
/// a conversation's images live inside it, so nothing is left behind holding
/// megabytes with no line referring to it any more. Taken under the same lock
/// as a save, so a write already in flight finishes into the directory rather
/// than half into a directory being taken away.
///
/// Only a person's own *Delete* reaches this. Nothing is ever removed on its
/// own — no retention sweep, no age limit — because that would be the app
/// throwing away work nobody asked it to.
pub fn delete(dir: &Path) -> std::io::Result<()> {
    let lock = writer_for(dir);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    match std::fs::remove_dir_all(dir) {
        // Already gone is the outcome asked for, not a failure to report.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The lock for one conversation directory, made on first use.
///
/// What it holds is the [`MetaWrite::read_at`] of the newest metadata
/// written there.
fn writer_for(dir: &Path) -> Arc<Mutex<u64>> {
    static WRITERS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<u64>>>>> = OnceLock::new();
    let map = WRITERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = map.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(dir.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(0)))
        .clone()
}
