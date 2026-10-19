use super::*;
use onehand_core::workflow::Template;
use std::path::PathBuf;

fn shipped(id: &str) -> Entry {
    let mut template = Template::blank(id);
    template.id = id.to_string();
    Entry {
        template: Ok(template),
        file: None,
    }
}

fn own(id: &str) -> Entry {
    let mut template = Template::blank(id);
    template.id = id.to_string();
    Entry {
        template: Ok(template),
        file: Some(PathBuf::from(format!("{id}.toml"))),
    }
}

#[test]
fn listed_puts_own_before_shipped() {
    let entries = [shipped("a"), shipped("b"), own("c"), own("d")];
    assert_eq!(listed(&entries), vec![2, 3, 0, 1]);
}

#[test]
fn listed_caps() {
    let entries: Vec<Entry> = (0..crate::workflow::TEMPLATES_SHOWN + 3)
        .map(|n| own(&n.to_string()))
        .collect();
    assert_eq!(listed(&entries).len(), crate::workflow::TEMPLATES_SHOWN);
}

#[test]
fn pick_of_unreadable_own_is_own() {
    let entry = Entry {
        template: Err("broken".to_string()),
        file: Some(PathBuf::from("x.toml")),
    };
    assert_eq!(pick_of(&entry), Some(Pick::Own(PathBuf::from("x.toml"))));
    assert_eq!(pick_of(&shipped("a")), Some(Pick::Shipped("a".to_string())));
}

#[test]
fn repick_finds_by_key_after_reorder() {
    let pick = Pick::Own(PathBuf::from("c.toml"));
    assert_eq!(repick(&[own("c"), shipped("a")], &pick), Some(0));
    assert_eq!(repick(&[shipped("a"), own("d"), own("c")], &pick), Some(2));
}

#[test]
fn repick_gone_is_none() {
    let entries = [own("c")];
    assert_eq!(repick(&entries, &Pick::Own(PathBuf::from("x.toml"))), None);
    assert_eq!(repick(&entries, &Pick::New), None);
}
