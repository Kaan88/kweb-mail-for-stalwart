//! M6 against the mock server: address books, contacts and the away message, through the `Core`
//! object the way the app uses it.
//!
//! The mock behaves like Stalwart 1.1.0 where it matters (see `support/account.rs`): it validates
//! almost nothing on write, so the core has to.

mod support;

use std::sync::Arc;

use kweb_core::{
    ffi::Core,
    model::{AwayMessage, Contact, ContactRow, Labeled},
    secrets::MemorySecrets,
    Error,
};
use serde_json::{json, Value};
use support::MockServer;
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    /// A mock server and a core signed in to it.
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _rt: rt, _dir: dir }
    }

    /// A rig with the cards of `cards()` on the server.
    fn with_cards() -> Rig {
        let rig = Rig::new();
        for card in cards() {
            rig.server.add_contact(card);
        }
        rig
    }

    fn names(&self, query: &str) -> Vec<String> {
        self.core.contacts(query.into()).unwrap().rows.into_iter().map(|r| r.name).collect()
    }
}

fn cards() -> Vec<Value> {
    vec![
        // c1
        json!({
            "name": { "full": "Zoë Özdemir", "components": [
                { "kind": "given", "value": "Zoë" }, { "kind": "surname", "value": "Özdemir" }] },
            "organizations": { "o1": { "name": "Acme" } },
            "emails": { "e1": { "address": "zoe@x.test", "contexts": { "work": true } } },
            "phones": { "p1": { "number": "+49 (30) 1234-567", "contexts": { "private": true } } },
        }),
        // c2
        json!({ "name": { "full": "Ann Lee" }, "emails": { "a": { "address": "ann@x.test" } } }),
        // c3: a company
        json!({ "kind": "org", "name": { "full": "" }, "organizations": { "o": { "name": "Kweb Ltd" } },
                "emails": { "a": { "address": "info@kweb.test" } } }),
        // c4: only a mail address
        json!({ "emails": { "a": { "address": "bo@x.test" } } }),
        // c5: name components and no full name
        json!({ "name": { "components": [
            { "kind": "given", "value": "Émile" }, { "kind": "surname", "value": "Zola" }] } }),
        // c6: not Latin
        json!({ "name": { "full": "山田 太郎" } }),
    ]
}

fn labeled(label: &str, value: &str) -> Labeled {
    Labeled { label: label.into(), value: value.into() }
}

fn contact(name: &str, email: &str) -> Contact {
    Contact {
        address_book_id: "b".into(),
        name: name.into(),
        given_name: String::new(),
        family_name: String::new(),
        organization: String::new(),
        emails: if email.is_empty() { Vec::new() } else { vec![labeled("", email)] },
        phones: Vec::new(),
        addresses: Vec::new(),
        notes: String::new(),
    }
}

fn refused(result: Result<String, Error>) -> String {
    match result {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ---- the list ----------------------------------------------------------------------------

#[test]
fn the_list_is_sorted_by_name_without_regard_to_case_and_accents() {
    let rig = Rig::with_cards();
    let list = rig.core.contacts(String::new()).unwrap();
    assert!(!list.from_cache);
    let names: Vec<&str> = list.rows.iter().map(|r| r.name.as_str()).collect();
    // The card with only an address is named by it, the one with only parts by them, the company
    // by its organization. Other scripts sort by code point, after the Latin letters.
    assert_eq!(names, ["Ann Lee", "bo@x.test", "Émile Zola", "Kweb Ltd", "Zoë Özdemir", "山田 太郎"]);
    assert_eq!(
        list.rows[4],
        ContactRow {
            id: "c1".into(),
            name: "Zoë Özdemir".into(),
            email: "zoe@x.test".into(),
            organization: "Acme".into()
        }
    );
    assert_eq!(
        list.rows[3],
        ContactRow {
            id: "c3".into(),
            name: "Kweb Ltd".into(),
            email: "info@kweb.test".into(),
            organization: "Kweb Ltd".into()
        }
    );
    assert_eq!(list.rows[5].email, "");
}

#[test]
fn the_list_is_searched_by_name_organization_mail_and_phone() {
    let rig = Rig::with_cards();
    assert_eq!(rig.names("ozdemir"), ["Zoë Özdemir"]);
    assert_eq!(rig.names("ÖZDEMİR zoe"), ["Zoë Özdemir"], "case, accents and the Turkish dotted I, every word");
    assert_eq!(rig.names("emile"), ["Émile Zola"]);
    assert_eq!(rig.names("acme"), ["Zoë Özdemir"], "the organization");
    assert_eq!(rig.names("kweb"), ["Kweb Ltd"]);
    assert_eq!(rig.names("x.test"), ["Ann Lee", "bo@x.test", "Zoë Özdemir"], "every mail address, in list order");
    assert_eq!(rig.names("太郎"), ["山田 太郎"]);
    assert_eq!(rig.names("30 1234"), ["Zoë Özdemir"], "a phone number by its digits");
    assert_eq!(rig.names("+49301234"), ["Zoë Özdemir"]);
    assert_eq!(rig.names("  "), rig.names(""));
    assert!(rig.names("zzz").is_empty());
    assert!(rig.names("ann 30").is_empty(), "every word must be found");
}

#[test]
fn a_server_without_contacts_has_an_empty_list_and_no_books() {
    let rig = Rig::new();
    rig.server.set_contacts_enabled(false);
    assert!(rig.core.contacts(String::new()).unwrap().rows.is_empty());
    assert!(rig.core.address_books().unwrap().is_empty());
    assert_eq!(rig.server.request_count("ContactCard/query") + rig.server.request_count("AddressBook/get"), 0);
}

#[test]
fn the_list_is_fetched_in_pages() {
    let rig = Rig::new();
    for n in 0..1200 {
        rig.server.add_contact(json!({ "name": { "full": format!("Person {n:04}") } }));
    }
    let list = rig.core.contacts(String::new()).unwrap();
    assert_eq!(list.rows.len(), 1200);
    assert_eq!((list.rows[0].name.as_str(), list.rows[1199].name.as_str()), ("Person 0000", "Person 1199"));
    assert!(rig.server.request_count("ContactCard/query") >= 3, "the mock allows 500 objects in a get");
}

// ---- the books ---------------------------------------------------------------------------

#[test]
fn the_address_books_are_listed_with_their_rights() {
    let rig = Rig::new();
    rig.server.add_address_book("s", "Shared", false);
    let books = rig.core.address_books().unwrap();
    let flags: Vec<(&str, bool, bool)> = books.iter().map(|b| (b.id.as_str(), b.is_default, b.may_write)).collect();
    assert_eq!(flags, [("b", true, true), ("s", false, false)]);
    assert_eq!(books[1].name, "Shared");
}

// ---- offline -----------------------------------------------------------------------------

#[test]
fn offline_the_list_the_contact_and_the_books_come_from_the_cache() {
    let rig = Rig::with_cards();
    rig.server.add_address_book("s", "Shared", false);
    let online = rig.core.contacts(String::new()).unwrap();
    let detail = rig.core.contact("c1".into()).unwrap();
    let books = rig.core.address_books().unwrap();

    rig.server.set_down(true);
    let offline = rig.core.contacts(String::new()).unwrap();
    assert!(offline.from_cache);
    assert_eq!(offline.rows, online.rows);
    assert_eq!(rig.core.contacts("OZDEMIR".into()).unwrap().rows.len(), 1, "searched in the cache too");
    assert_eq!(rig.core.contact("c1".into()).unwrap(), detail);
    assert_eq!(rig.core.address_books().unwrap(), books);
    assert!(rig.core.contact("c2".into()).is_ok(), "every card came with the list");
    assert!(matches!(rig.core.contact("zz".into()), Err(Error::Temporary { .. })), "not cached, not known");
    // A change cannot be made offline.
    assert!(matches!(rig.core.save_contact(None, contact("New", "")), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.delete_contact("c1".into()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.away_message(), Err(Error::Temporary { .. })));

    rig.server.set_down(false);
    assert!(!rig.core.contacts(String::new()).unwrap().from_cache);
}

#[test]
fn offline_with_nothing_cached_is_the_servers_error_and_not_an_empty_list() {
    let rig = Rig::with_cards();
    rig.server.set_down(true);
    assert!(matches!(rig.core.contacts(String::new()), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.address_books(), Err(Error::Temporary { .. })));
    assert!(matches!(rig.core.contact("c1".into()), Err(Error::Temporary { .. })));
}

#[test]
fn the_contact_screen_knows_whether_it_may_be_changed() {
    let rig = Rig::new();
    rig.server.add_address_book("s", "Shared", false);
    let mine = rig.server.add_contact(json!({ "name": { "full": "Mine" } }));
    let theirs = rig.server.add_contact(json!({ "name": { "full": "Theirs" }, "addressBookIds": { "s": true } }));
    let detail = rig.core.contact(mine.clone()).unwrap();
    assert!(detail.may_write);
    assert_eq!(
        (detail.id.as_str(), detail.contact.address_book_id.as_str(), detail.contact.name.as_str()),
        ("c1", "b", "Mine")
    );
    assert!(!rig.core.contact(theirs.clone()).unwrap().may_write);
    // Nothing is written into a book that is read-only.
    let mut c = rig.core.contact(theirs.clone()).unwrap().contact;
    c.notes = "x".into();
    assert!(refused(rig.core.save_contact(Some(theirs), c)).contains("read-only"));
    assert!(matches!(rig.core.contact("gone".into()), Err(Error::Invalid { .. })));
}

// ---- writing -----------------------------------------------------------------------------

#[test]
fn a_contact_is_made_read_back_changed_and_deleted() {
    let rig = Rig::new();
    let mut c = contact("Zoë Özdemir", "zoe@x.test");
    c.given_name = "Zoë".into();
    c.family_name = "Özdemir".into();
    c.organization = "Acme".into();
    c.emails.push(labeled("private", "zoe.home@x.test"));
    c.phones = vec![labeled("work", "+49 30 1234")];
    c.addresses = vec![labeled("", "Main St 1\nBerlin")];
    c.notes = "Met at the fair".into();
    let id = rig.core.save_contact(None, c.clone()).unwrap();

    // The server holds a card that says so, and the app reads the same back.
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(card["addressBookIds"], json!({ "b": true }));
    assert_eq!(card["name"]["full"], "Zoë Özdemir");
    assert_eq!(card["emails"]["e2"]["contexts"], json!({ "private": true }));
    let detail = rig.core.contact(id.clone()).unwrap();
    assert_eq!(detail.contact, c);
    assert!(detail.may_write);
    // It is in the list and on offer as a recipient, without the list being fetched again.
    rig.server.set_down(true);
    assert_eq!(rig.names(""), ["Zoë Özdemir"]);
    rig.server.set_down(false);

    // A change: only what was edited is sent.
    let mut changed = detail.contact;
    changed.name = "Zoe Z".into();
    changed.emails.remove(0);
    changed.phones[0].value = "+49 30 9999".into();
    changed.notes = String::new();
    assert_eq!(rig.core.save_contact(Some(id.clone()), changed.clone()).unwrap(), id);
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(card["name"]["full"], "Zoe Z");
    assert_eq!(card["name"]["components"][0]["value"], "Zoë", "the parts were not edited");
    assert_eq!(
        card["emails"],
        json!({ "e2": { "@type": "EmailAddress", "address": "zoe.home@x.test", "contexts": { "private": true } } })
    );
    assert_eq!(card["phones"]["p1"]["number"], "+49 30 9999");
    assert!(card.get("notes").is_none(), "a note that was cleared is gone");
    assert_eq!(rig.core.contact(id.clone()).unwrap().contact, changed);
    assert_eq!(rig.names("zoe z"), ["Zoe Z"]);

    rig.core.delete_contact(id.clone()).unwrap();
    assert!(rig.server.contact_cards().is_empty());
    assert!(rig.names("").is_empty());
    assert!(matches!(rig.core.contact(id.clone()), Err(Error::Invalid { .. })));
    rig.core.delete_contact(id).unwrap();
}

#[test]
fn a_contact_can_be_moved_to_another_book() {
    let rig = Rig::with_cards();
    rig.server.add_address_book("w", "Work", true);
    let mut c = rig.core.contact("c2".into()).unwrap().contact;
    c.address_book_id = "w".into();
    rig.core.save_contact(Some("c2".into()), c).unwrap();
    assert_eq!(rig.server.contact_card("c2").unwrap()["addressBookIds"], json!({ "w": true }));
    assert_eq!(rig.core.contact("c2".into()).unwrap().contact.address_book_id, "w");
}

#[test]
fn a_save_keeps_what_the_app_does_not_own() {
    let rig = Rig::new();
    let id = rig.server.add_contact(json!({
        "kind": "individual", "uid": "urn:uuid:1",
        "name": { "full": "Ann Lee", "isOrdered": true, "components": [
            { "kind": "title", "value": "Dr." }, { "kind": "given", "value": "Ann" }, { "kind": "surname", "value": "Lee" }] },
        "nicknames": { "k": { "name": "Annie" } },
        "anniversaries": { "x": { "kind": "birth", "date": { "year": 1990 } } },
        "emails": { "e1": { "address": "ann@x.test", "pref": 1, "contexts": { "work": true, "billing": true } },
                    "e2": { "address": "ann2@x.test" } },
        "phones": { "p1": { "number": "0170 1", "features": { "mobile": true } }, "p2": { "uri": "tel:+491" } },
        "addresses": { "a1": { "components": [{ "kind": "locality", "value": "Berlin" }], "isOrdered": true } },
        "media": { "m": { "kind": "photo", "uri": "https://x.test/a.png" } },
        "x:custom": 7
    }));
    let before = rig.server.contact_card(&id).unwrap();
    let mut c = rig.core.contact(id.clone()).unwrap().contact;

    // Saved as it is read: nothing is sent at all.
    rig.core.save_contact(Some(id.clone()), c.clone()).unwrap();
    assert_eq!(rig.server.request_count("ContactCard/set"), 0);
    assert_eq!(rig.server.contact_card(&id).unwrap(), before);

    // Changed: the rest of the card is as it was.
    c.family_name = "Leigh".into();
    c.emails[1].label = "work".into();
    c.emails.push(labeled("", "new@x.test"));
    c.phones[0].value = "0170 2".into();
    rig.core.save_contact(Some(id.clone()), c).unwrap();
    let after = rig.server.contact_card(&id).unwrap();
    for kept in ["kind", "uid", "nicknames", "anniversaries", "addresses", "media", "x:custom", "id", "addressBookIds"]
    {
        assert_eq!(after[kept], before[kept], "{kept}");
    }
    assert_eq!(after["name"]["full"], "Ann Lee", "not edited");
    assert_eq!(after["name"]["components"][0], before["name"]["components"][0], "the title stays");
    assert_eq!(after["name"]["components"][2]["value"], "Leigh");
    assert_eq!(
        after["emails"]["e1"], before["emails"]["e1"],
        "an entry that did not change keeps its id and everything in it"
    );
    assert_eq!(after["emails"]["e2"], json!({ "address": "ann2@x.test", "contexts": { "work": true } }));
    assert_eq!(after["emails"]["e3"]["address"], "new@x.test");
    assert_eq!(after["phones"]["p1"], json!({ "number": "0170 2", "features": { "mobile": true } }));
    assert_eq!(after["phones"]["p2"], before["phones"]["p2"], "a phone the app cannot show stays");
}

#[test]
fn a_card_with_a_bad_address_from_elsewhere_can_still_be_changed() {
    let rig = Rig::new();
    let id = rig.server.add_contact(json!({ "name": { "full": "Old" }, "emails": { "e": { "address": "junk" } } }));
    let mut c = rig.core.contact(id.clone()).unwrap().contact;
    c.notes = "now with a note".into();
    rig.core.save_contact(Some(id.clone()), c.clone()).unwrap();
    assert_eq!(rig.server.contact_card(&id).unwrap()["notes"]["n1"]["note"], "now with a note");
    // A new bad one is not let in.
    c.emails.push(labeled("", "also junk"));
    assert!(refused(rig.core.save_contact(Some(id), c)).contains("also junk"));
}

#[test]
fn what_is_wrong_is_refused_in_words_before_anything_is_sent() {
    let rig = Rig::new();
    rig.server.add_address_book("r", "Shared", false);
    let save = |c: Contact| refused(rig.core.save_contact(None, c));
    assert_eq!(save(contact("", "")), "Give the contact a name or a mail address.");
    assert_eq!(save(contact("  ", "  ")), "Give the contact a name or a mail address.");
    for bad in ["ann", "@x.test", "ann@", "a@b@c", "an n@x.test", "ann@x .test"] {
        assert!(save(contact("Ann", bad)).contains(&format!("\"{bad}\" is not a mail address")), "{bad}");
    }
    let mut c = contact("Ann", "ann@x.test");
    c.address_book_id = String::new();
    assert_eq!(save(c.clone()), "Choose an address book.");
    c.address_book_id = "nope".into();
    assert_eq!(save(c.clone()), "This address book does not exist any more.");
    c.address_book_id = "r".into();
    assert_eq!(save(c), "The address book \"Shared\" is read-only.");
    // A change to a contact the server does not have.
    assert!(refused(rig.core.save_contact(Some("zz".into()), contact("Ann", ""))).contains("no longer exists"));
    assert_eq!(rig.server.request_count("ContactCard/set"), 0);
    assert!(rig.server.contact_cards().is_empty());
}

#[test]
fn blank_entries_are_dropped_not_refused() {
    let rig = Rig::new();
    let mut c = contact(" Ann ", "");
    c.emails = vec![labeled("", " "), labeled("work", " ann@x.test ")];
    c.phones = vec![labeled("", "")];
    c.addresses = vec![labeled("", " \n ")];
    c.organization = "  ".into();
    c.notes = " \n".into();
    let id = rig.core.save_contact(None, c).unwrap();
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(card["name"]["full"], "Ann");
    assert_eq!(
        card["emails"],
        json!({ "e1": { "@type": "EmailAddress", "address": "ann@x.test", "contexts": { "work": true } } })
    );
    for absent in ["phones", "addresses", "organizations", "notes"] {
        assert!(card.get(absent).is_none(), "{absent}");
    }
}

#[test]
fn a_contact_changed_elsewhere_meanwhile_is_read_again_once() {
    let rig = Rig::new();
    let id = rig.server.add_contact(json!({ "name": { "full": "Ann" }, "organizations": { "o": { "name": "Acme" } } }));
    let mut c = rig.core.contact(id.clone()).unwrap().contact;
    c.notes = "mine".into();
    // Someone else adds a nickname between the read and the write: it stays, and mine is kept too.
    // (What the app owns is as the user saved it, whoever changed it meanwhile.)
    rig.server.interfere_with_contact(&id, "nicknames", json!({ "k": { "name": "Annie" } }));
    rig.core.save_contact(Some(id.clone()), c.clone()).unwrap();
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(
        (card["nicknames"]["k"]["name"].as_str(), card["notes"]["n1"]["note"].as_str()),
        (Some("Annie"), Some("mine"))
    );
    assert_eq!(rig.server.request_count("ContactCard/set"), 2, "the first was refused as of an old state");

    // Twice in a row is too much.
    c.notes = "again".into();
    rig.server.interfere_with_contact(&id, "x:a", json!(1));
    rig.server.interfere_with_contact(&id, "x:b", json!(2));
    assert!(matches!(rig.core.save_contact(Some(id.clone()), c), Err(Error::Temporary { .. })));
    assert_eq!(rig.server.contact_card(&id).unwrap()["notes"]["n1"]["note"], "mine");
}

#[test]
fn the_recipient_suggestions_see_a_new_contact_and_lose_a_deleted_one() {
    let rig = Rig::new();
    let suggested = |text: &str| -> Vec<(String, String)> {
        rig.core.suggest_recipients(text.into(), 10).unwrap().into_iter().map(|a| (a.name, a.email)).collect()
    };
    assert!(suggested("zoe").is_empty());
    let mut c = contact("Zoë Özdemir", "zoe@x.test");
    c.emails.push(labeled("private", "zoe.home@x.test"));
    let id = rig.core.save_contact(None, c).unwrap();
    assert_eq!(
        suggested("zoe"),
        [("Zoë Özdemir", "zoe@x.test"), ("Zoë Özdemir", "zoe.home@x.test")]
            .map(|(n, e)| (n.to_string(), e.to_string()))
    );
    // A new address on the same contact.
    let mut changed = rig.core.contact(id.clone()).unwrap().contact;
    changed.emails.remove(1);
    changed.emails.push(labeled("", "zoe.new@x.test"));
    rig.core.save_contact(Some(id.clone()), changed).unwrap();
    assert_eq!(suggested("zoe").len(), 2);
    assert_eq!(suggested("home").len(), 0);
    assert_eq!(suggested("zoe.new")[0].1, "zoe.new@x.test");
    // The list the user opens is the same people.
    assert_eq!(rig.names("zoe"), ["Zoë Özdemir"]);
    rig.core.delete_contact(id).unwrap();
    assert!(suggested("zoe").is_empty());
    // The cards a list fetch brings are suggested too, a card with only an address by the address.
    rig.server.add_contact(json!({ "emails": { "a": { "address": "bo@x.test" } } }));
    rig.core.contacts(String::new()).unwrap();
    assert_eq!(suggested("bo@"), [("".to_string(), "bo@x.test".to_string())]);
}

#[test]
fn contacts_need_an_account() {
    let rig = Rig::new();
    rig.core.sign_out().unwrap();
    assert!(matches!(rig.core.address_books(), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.contacts(String::new()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.contact("c1".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.save_contact(None, contact("A", "")), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.delete_contact("c1".into()), Err(Error::SignInNeeded { .. })));
    assert!(matches!(rig.core.away_message(), Err(Error::SignInNeeded { .. })));
}

#[test]
fn what_another_client_changed_since_the_form_was_opened_is_not_reverted() {
    let rig = Rig::new();
    let id = rig.server.add_contact(json!({
        "name": { "full": "Ann" }, "emails": { "e1": { "address": "a@x.test" } },
        "phones": { "p1": { "number": "111" } }, "organizations": { "o": { "name": "Acme" } }
    }));
    let mut form = rig.core.contact(id.clone()).unwrap().contact;
    // Meanwhile the webmail adds a mail address and a phone number and changes the organization.
    rig.server.set_contact_property(
        &id,
        "emails",
        json!({ "e1": { "address": "a@x.test" }, "e2": { "address": "b@x.test" } }),
    );
    rig.server.set_contact_property(&id, "phones", json!({ "p1": { "number": "111" }, "p2": { "number": "222" } }));
    rig.server.set_contact_property(&id, "organizations", json!({ "o": { "name": "Acme Corp" } }));
    // The user changes only the note and adds an address of their own.
    form.notes = "mine".into();
    form.emails.push(labeled("", "c@x.test"));
    rig.core.save_contact(Some(id.clone()), form).unwrap();
    let card = rig.server.contact_card(&id).unwrap();
    let values = |map: &str, field: &str| -> Vec<String> {
        card[map].as_object().unwrap().values().map(|e| e[field].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(values("emails", "address"), ["a@x.test", "b@x.test", "c@x.test"]);
    assert_eq!(values("phones", "number"), ["111", "222"]);
    assert_eq!(card["organizations"]["o"]["name"], "Acme Corp", "the user did not edit it");
    assert_eq!(card["notes"]["n1"]["note"], "mine");

    // What the user removed is removed, also when the card has changed around it.
    let mut form = rig.core.contact(id.clone()).unwrap().contact;
    form.emails.retain(|e| e.value != "a@x.test");
    rig.server.set_contact_property(&id, "phones", json!({ "p1": { "number": "111" } }));
    rig.core.save_contact(Some(id.clone()), form).unwrap();
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(card["emails"].as_object().unwrap().len(), 2);
    assert!(card["emails"].as_object().unwrap().values().all(|e| e["address"] != "a@x.test"));
    assert_eq!(card["phones"], json!({ "p1": { "number": "111" } }), "the one somebody removed stays removed");
}

#[test]
fn a_form_filled_from_the_cache_does_not_write_the_old_values_back() {
    let rig = Rig::new();
    let id =
        rig.server.add_contact(json!({ "name": { "full": "Ann" }, "emails": { "e1": { "address": "a@x.test" } } }));
    rig.core.contacts(String::new()).unwrap();
    rig.server.set_down(true);
    let mut form = rig.core.contact(id.clone()).unwrap().contact; // days old, from the cache
    rig.server.set_down(false);
    rig.server.set_contact_property(
        &id,
        "emails",
        json!({ "e1": { "address": "a@x.test" }, "e2": { "address": "b@x.test" } }),
    );
    form.notes = "mine".into();
    rig.core.save_contact(Some(id.clone()), form).unwrap();
    let card = rig.server.contact_card(&id).unwrap();
    assert_eq!(
        (card["emails"]["e2"]["address"].as_str(), card["notes"]["n1"]["note"].as_str()),
        (Some("b@x.test"), Some("mine"))
    );
}

#[test]
fn the_list_asks_for_no_media_and_a_search_does_not_ask_the_server_at_all() {
    let rig = Rig::new();
    rig.server.add_contact(json!({
        "name": { "full": "Ann Lee" }, "emails": { "a": { "address": "ann@x.test" } },
        "media": { "m": { "kind": "photo", "uri": "data:image/png;base64,AAAA" } }
    }));
    rig.server.add_contact(json!({ "name": { "full": "Bo" } }));
    // A card that was only saved does not make a cache that can answer a search.
    assert_eq!(rig.names("ann"), ["Ann Lee"]);
    assert_eq!(rig.server.request_count("ContactCard/query"), 1, "the first search lists");
    let asked = rig.server.contact_get_properties();
    assert!(!asked.is_empty());
    for properties in asked {
        let names: Vec<&str> =
            properties.as_array().expect("a listing names its properties").iter().filter_map(Value::as_str).collect();
        assert!(names.contains(&"emails") && !names.contains(&"media"), "{names:?}");
    }
    // Now they are in the cache: a search is answered from it, a blank query lists again.
    rig.server.add_contact(json!({ "name": { "full": "Anna New" } }));
    assert_eq!(rig.names("ann"), ["Ann Lee"]);
    assert_eq!(rig.server.request_count("ContactCard/query"), 1);
    assert_eq!(rig.names("").len(), 3);
    assert_eq!(rig.server.request_count("ContactCard/query"), 2);
    assert_eq!(rig.names("ann"), ["Ann Lee", "Anna New"]);
}

#[test]
fn a_cache_with_only_a_saved_contact_does_not_answer_a_search() {
    let rig = Rig::with_cards();
    rig.core.save_contact(None, contact("Ann Mine", "")).unwrap();
    assert_eq!(rig.names("ann"), ["Ann Lee", "Ann Mine"], "the list was never fetched: the server is asked");
}

#[test]
fn the_list_is_complete_when_the_server_gives_fewer_ids_than_asked_for() {
    let rig = Rig::new();
    for n in 0..250 {
        rig.server.add_contact(json!({ "name": { "full": format!("Person {n:03}") } }));
    }
    rig.server.set_contact_query_limit(100);
    let list = rig.core.contacts(String::new()).unwrap();
    assert_eq!(list.rows.len(), 250);
    assert_eq!(rig.server.request_count("ContactCard/query"), 3);
}

#[test]
fn the_first_entry_of_a_card_is_the_preferred_one_then_the_lowest_key() {
    let rig = Rig::new();
    let id = rig.server.add_contact(json!({
        "emails": {
            "work": { "address": "old@x.test" }, "zhome": { "address": "main@x.test", "pref": 1 },
            "e10": { "address": "ten@x.test" }, "e2": { "address": "two@x.test" }
        }
    }));
    let contact = rig.core.contact(id).unwrap().contact;
    let addresses: Vec<&str> = contact.emails.iter().map(|e| e.value.as_str()).collect();
    assert_eq!(addresses, ["main@x.test", "two@x.test", "ten@x.test", "old@x.test"]);
    assert_eq!(contact.name, "main@x.test");
    assert_eq!(rig.core.contacts(String::new()).unwrap().rows[0].email, "main@x.test");
}

#[test]
fn a_contact_the_server_no_longer_has_leaves_the_cache_and_the_suggestions() {
    let rig = Rig::with_cards();
    rig.core.contacts(String::new()).unwrap();
    let suggested = |text: &str| rig.core.suggest_recipients(text.into(), 10).unwrap().len();
    assert_eq!(suggested("ann@"), 1);
    rig.server.remove_contact("c2"); // Ann Lee, in the webmail
    rig.server.remove_contact("c4"); // bo@x.test
    assert!(matches!(rig.core.contact("c2".into()), Err(Error::Invalid { .. })));
    assert!(refused(rig.core.save_contact(Some("c4".into()), contact("Bo", ""))).contains("no longer exists"));
    assert_eq!(suggested("ann@") + suggested("bo@"), 0);
    rig.server.set_down(true);
    let names = rig.names("");
    assert!(!names.contains(&"Ann Lee".to_string()) && !names.contains(&"bo@x.test".to_string()), "{names:?}");
    assert!(matches!(rig.core.contact("c2".into()), Err(Error::Temporary { .. })));
}

// ---- the away message --------------------------------------------------------------------

fn away(enabled: bool, from: Option<i64>, until: Option<i64>, text: &str) -> AwayMessage {
    AwayMessage { is_enabled: enabled, from, until, subject: "Away".into(), text: text.into() }
}

#[test]
fn the_away_message_is_read_switched_on_with_dates_and_off_again() {
    let rig = Rig::new();
    let off = rig.core.away_message().unwrap();
    assert_eq!(
        off,
        AwayMessage { is_enabled: false, from: None, until: None, subject: String::new(), text: String::new() }
    );

    // 2026-12-01 00:00 UTC to 2026-12-10 12:30 UTC.
    let on = away(true, Some(1_796_083_200), Some(1_796_898_600), "I am away until the 10th.");
    rig.core.set_away_message(on.clone()).unwrap();
    assert_eq!(
        rig.server.vacation(),
        json!({
            "id": "singleton", "isEnabled": true, "fromDate": "2026-12-01T00:00:00Z", "toDate": "2026-12-10T10:30:00Z",
            "subject": "Away", "textBody": "I am away until the 10th.", "htmlBody": null
        })
    );
    assert_eq!(rig.core.away_message().unwrap(), on);

    // Off again, dates cleared, the text kept for the next time.
    rig.core.set_away_message(away(false, None, None, "I am away until the 10th.")).unwrap();
    let state = rig.server.vacation();
    assert_eq!(
        (&state["isEnabled"], &state["fromDate"], &state["toDate"]),
        (&json!(false), &Value::Null, &Value::Null)
    );
    assert_eq!(state["textBody"], "I am away until the 10th.");
    assert!(!rig.core.away_message().unwrap().is_enabled);
}

#[test]
fn a_message_another_client_wrote_as_html_is_read_as_text_and_kept_unless_the_text_changes() {
    let rig = Rig::new();
    rig.server
        .set_vacation(json!({ "isEnabled": true, "htmlBody": "<p>Back <b>soon</b></p><p>Ann</p>", "subject": "Hi" }));
    let mut m = rig.core.away_message().unwrap();
    assert_eq!((m.is_enabled, m.text.as_str(), m.subject.as_str()), (true, "Back soon\nAnn", "Hi"));
    // Only the switch is turned off: the formatted version and the subject are as they were.
    m.is_enabled = false;
    rig.core.set_away_message(m.clone()).unwrap();
    let state = rig.server.vacation();
    assert_eq!(
        (&state["isEnabled"], state["htmlBody"].as_str(), state["subject"].as_str(), &state["textBody"]),
        (&json!(false), Some("<p>Back <b>soon</b></p><p>Ann</p>"), Some("Hi"), &Value::Null)
    );
    // A text the user wrote replaces it, and the two cannot disagree.
    m.text = "Back on Monday".into();
    rig.core.set_away_message(m).unwrap();
    let state = rig.server.vacation();
    assert_eq!((state["textBody"].as_str(), &state["htmlBody"]), (Some("Back on Monday"), &Value::Null));
}

#[test]
fn a_wrong_away_message_is_refused_before_it_is_sent() {
    let rig = Rig::new();
    let set = |m: AwayMessage| match rig.core.set_away_message(m) {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert_eq!(set(away(true, Some(200), Some(100), "x")), "The away message has to end after it starts.");
    assert_eq!(set(away(true, Some(100), Some(100), "x")), "The away message has to end after it starts.");
    assert_eq!(set(away(true, None, None, "  ")), "Write the message that is sent while you are away.");
    assert_eq!(rig.server.request_count("VacationResponse/set"), 0);
    assert_eq!(rig.server.vacation()["isEnabled"], false);
    // Off needs no text, and one date alone is fine.
    rig.core.set_away_message(away(false, None, None, "")).unwrap();
    rig.core.set_away_message(away(true, None, Some(1_796_898_600), "x")).unwrap();
}

#[test]
fn a_server_without_an_away_message_says_so() {
    let rig = Rig::new();
    rig.server.set_vacation_enabled(false);
    assert!(matches!(rig.core.away_message(), Err(Error::Invalid { .. })));
    assert!(matches!(rig.core.set_away_message(away(false, None, None, "")), Err(Error::Invalid { .. })));
    assert_eq!(rig.server.request_count("VacationResponse/get"), 0);
}
