//! Writing a config back into the file it was read from.
//!
//! **Only what changed is written.** The file is a document a person edits as
//! well as one the app saves, and a newer build writes keys an older one does
//! not know: rewriting it from the typed config dropped every comment and every
//! such key at each save of anything. So the save is a difference — the config
//! as read against the config after the edit — applied to the document, and a
//! key the two agree on, or that neither knows, is never touched.

use toml_edit::{DocumentMut, Item, TableLike};

/// Put into `doc` `after`'s value at every key where `before` and `after`
/// differ, and leave everything else in it as it was.
pub(super) fn merge(doc: &mut DocumentMut, before: &toml::Table, after: &toml::Table) {
    let mut next = 0;
    each_table(doc.as_item_mut(), &mut |table| {
        next = next.max(table.position().map_or(0, |at| at + 1));
    });
    merge_into(doc.as_table_mut(), before, after, &mut next);
}

fn merge_into(
    target: &mut dyn TableLike,
    before: &toml::Table,
    after: &toml::Table,
    next: &mut usize,
) {
    for key in before.keys().filter(|key| !after.contains_key(*key)) {
        target.remove(key);
    }
    for (key, value) in after {
        let was = before.get(key);
        if was == Some(value) {
            continue;
        }
        // A table is merged key by key wherever the file already holds one,
        // in whichever shape it was written: a header, dotted keys, inline.
        if let toml::Value::Table(now) = value {
            if let Some(table) = target.get_mut(key).and_then(Item::as_table_like_mut) {
                let empty = toml::Table::new();
                let was = match was {
                    Some(toml::Value::Table(was)) => was,
                    _ => &empty,
                };
                merge_into(table, was, now, next);
                continue;
            }
        }
        // ponytail: an array, `[[agents]]` included, is replaced whole when it
        // differs, so a key a newer build wrote inside one agent entry is lost
        // when the list changes. Merge entries by position once `AgentSpec`
        // gains a field.
        let mut item = item_of(key, value);
        // A table is drawn where its position says, and one read from the
        // scratch document has that document's: numbered after every table
        // already here, it lands at the end instead of among them.
        each_table(&mut item, &mut |table| {
            table.set_position(*next);
            *next += 1;
        });
        // Replaced where it stands rather than inserted anew: the key keeps
        // its own decor, which is where the comment above a line lives, and
        // the value keeps the comment after it. A list of tables keeps each
        // entry's, by position, which is where the comments around it live.
        match target.get_mut(key) {
            Some(slot) => {
                match (&*slot, &mut item) {
                    (Item::Value(old), Item::Value(new)) => *new.decor_mut() = old.decor().clone(),
                    (Item::Table(old), Item::Table(new)) => *new.decor_mut() = old.decor().clone(),
                    (Item::ArrayOfTables(old), Item::ArrayOfTables(new)) => {
                        for (old, new) in old.iter().zip(new.iter_mut()) {
                            *new.decor_mut() = old.decor().clone();
                        }
                    }
                    _ => {}
                }
                *slot = item;
            }
            None => {
                target.insert(key, item);
            }
        }
    }
}

/// Call `visit` on every table under `item`, in the order the document
/// writes them.
fn each_table(item: &mut Item, visit: &mut dyn FnMut(&mut toml_edit::Table)) {
    let children = |table: &mut toml_edit::Table, visit: &mut dyn FnMut(&mut toml_edit::Table)| {
        for (_, child) in table.iter_mut() {
            each_table(child, visit);
        }
    };
    match item {
        Item::Table(table) => {
            visit(table);
            children(table, visit);
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                visit(table);
                children(table, visit);
            }
        }
        Item::None | Item::Value(_) => {}
    }
}

/// `value` as the document would hold it under `key`: written out and read
/// back, so a table comes back as a `[table]` and a list of tables as
/// `[[key]]`, never squeezed onto one line.
fn item_of(key: &str, value: &toml::Value) -> Item {
    let mut wrap = toml::Table::new();
    wrap.insert(key.to_string(), value.clone());
    let text = toml::to_string_pretty(&wrap).expect("a value read from TOML writes as TOML");
    let mut doc: DocumentMut = text.parse().expect("TOML just written reads back");
    doc.remove(key).expect("the key just written is there")
}

#[cfg(test)]
mod tests;
