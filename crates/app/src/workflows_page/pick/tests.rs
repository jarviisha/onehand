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
    assert_eq!(listed(&entries, "").shown, vec![2, 3, 0, 1]);
}

#[test]
fn listed_caps_after_filtering() {
    let mut entries: Vec<Entry> = (0..crate::workflow::TEMPLATES_SHOWN + 3)
        .map(|n| own(&format!("w{n}")))
        .collect();
    entries.push(own("needle"));
    let all = listed(&entries, "");
    assert_eq!(all.shown.len(), crate::workflow::TEMPLATES_SHOWN);
    assert_eq!(all.matched, crate::workflow::TEMPLATES_SHOWN + 4);
    let found = listed(&entries, "NEEDLE");
    assert_eq!(found.matched, 1);
    assert_eq!(found.shown, vec![entries.len() - 1]);
}

#[test]
fn matches_name_description_and_unreadable_file_name() {
    let mut described = own("a");
    if let Ok(template) = described.template.as_mut() {
        template.description = "Opens a pull request".into();
    }
    assert!(matches(&described, "  pull "));
    assert!(!matches(&described, "zzz"));
    let broken = Entry {
        template: Err("bad".into()),
        file: Some(PathBuf::from("old-release.toml")),
    };
    assert!(matches(&broken, "release"));
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

#[test]
fn card_acts_by_entry() {
    let a = card_acts(&own("a"));
    assert!(a.duplicate && a.export && a.delete);
    let s = card_acts(&shipped("s"));
    assert!(s.duplicate && s.export && !s.delete);
    let broken = Entry {
        template: Err("bad".into()),
        file: Some(PathBuf::from("x.toml")),
    };
    let b = card_acts(&broken);
    assert!(!b.duplicate && !b.export && b.delete);
}
