use super::*;
use futures::StreamExt as _;

fn standing_of(token: &str) -> Standing {
    standing(&Peer::Session(token.to_string()))
}

#[test]
fn a_granted_session_stands_with_its_set_until_switched_on() {
    let uid = 900_002;
    let token = Token::mint();
    token.claim(uid);
    let name = token.0.clone();
    grant(
        uid,
        &onehand_core::task::Grant {
            tools: vec!["list_issues".into()],
            labels: vec!["auto".into()],
        },
    );
    assert!(matches!(standing_of(&name), Standing::Only(set) if set == ["list_issues"]));
    assert_eq!(sessions().labels[&uid], ["auto"]);
    switch(uid, true);
    assert!(matches!(standing_of(&name), Standing::On));
    switch(uid, false);
    assert!(matches!(standing_of(&name), Standing::Only(_)));
    forget(uid);
    assert!(matches!(standing_of(&name), Standing::Off));
    assert!(!sessions().labels.contains_key(&uid));
}

#[test]
fn a_token_speaks_for_its_session_until_it_goes() {
    let uid = 900_001;
    let token = Token::mint();
    let name = token.0.clone();
    // A warm adapter's: known, and offered nothing.
    assert!(matches!(standing_of(&name), Standing::Off));
    token.claim(uid);
    assert!(matches!(standing_of(&name), Standing::Off));

    let (told, mut heard) = futures::channel::mpsc::unbounded();
    let (gone, _) = futures::channel::mpsc::unbounded::<String>();
    sessions()
        .conns
        .entry(name.clone())
        .or_default()
        .extend([told, gone]);
    switch(uid, true);
    assert!(switched_on(uid));
    assert!(matches!(standing_of(&name), Standing::On));
    let note = futures::executor::block_on(heard.next()).unwrap();
    assert!(note.contains("list_changed"), "{note}");
    // The connection whose other end went is dropped, the live one kept.
    assert_eq!(sessions().conns[&name].len(), 1);
    // Switching to where it is already tells nobody.
    switch(uid, true);
    assert!(heard.try_recv().is_err());

    forget(uid);
    assert!(!switched_on(uid));
    drop(token);
    assert!(matches!(standing_of(&name), Standing::Refused(_)));
    assert!(!sessions().conns.contains_key(&name));
}

#[test]
fn a_grant_tells_an_agent_that_listed_its_tools_before() {
    let uid = 900_003;
    let token = Token::mint();
    let name = token.0.clone();
    // A warm adapter's agent connected and listed nothing before the claim.
    let (told, mut heard) = futures::channel::mpsc::unbounded();
    sessions().conns.entry(name.clone()).or_default().push(told);
    token.claim(uid);
    let given = onehand_core::task::Grant {
        tools: vec!["create_issue".into()],
        labels: Vec::new(),
    };
    grant(uid, &given);
    let note = futures::executor::block_on(heard.next()).unwrap();
    assert!(note.contains("list_changed"), "{note}");
    // The same grant again, a Resume's, tells nobody.
    grant(uid, &given);
    assert!(heard.try_recv().is_err());
    forget(uid);
}
