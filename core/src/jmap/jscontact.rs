//! Contact cards (JSContact, RFC 9553) as the app reads and writes them: pure functions over
//! JSON, tested with fixtures.
//!
//! What Stalwart 1.1.0 does, found against the real server; the code follows the server:
//!
//! - A card is in `addressBookIds`. A create without one is `invalidProperties`, and so is
//!   taking a card out of its last book. The card's book here is the first of them.
//! - Nothing else is validated on write: a mail address of "junk" or a card with no name at all
//!   is stored (`jscontact::validate` does it). A card made with only `addressBookIds` comes
//!   back with `name: {"full": ""}`.
//! - A patch into an entry that is there (`emails/e1/address`) or a new key of a map that is
//!   there (`emails/e3`) works; into a map or entry that is not there it is `invalidPatch`.
//!   A whole map works, `null` or `{}` removes it. So the maps are written whole, with the
//!   entries that did not change kept as they are (the ids stay), and the name is patched
//!   inside when the card has one.
//! - `ContactCard/get` adds `@type`, `isOrdered` and the like; they are kept when an entry is
//!   changed and not required when one is made.
//! - Maps come in key order: the order of a JSON object means nothing, so what the app calls the
//!   first entry is the one with the lowest `pref`, then the lowest key (`e2` before `e10`).
//! - A change is a three-way one: the form's values against the contact as the form was opened
//!   (`base`) say what the user changed, and only that is written onto the card as it is now.

use std::cmp::Ordering;

use serde_json::{json, Map, Value};

use super::send::pointer_segment;
use crate::{
    model::{AddressBook, Contact, Labeled},
    Error, Result,
};

/// The name of a card that has no name, no organization, no mail address and no phone.
const NO_NAME: &str = "(no name)";
/// The contexts that become a label of their own; any other label is the entry's `label` text.
const CONTEXTS: [&str; 2] = ["work", "private"];
/// The parts of a postal address that go on a line after the street.
const TOWN_PARTS: [&str; 4] = ["postcode", "locality", "region", "country"];

/// A map of which the app shows the first text: where it is, which field, what the objects are called.
struct TextField {
    map: &'static str,
    field: &'static str,
    type_name: &'static str,
    prefix: &'static str,
}

const ORGANIZATION: TextField =
    TextField { map: "organizations", field: "name", type_name: "Organization", prefix: "o" };
const NOTE: TextField = TextField { map: "notes", field: "note", type_name: "Note", prefix: "n" };

/// A list of values with labels on a card.
#[derive(Clone, Copy)]
enum Kind {
    Email,
    Phone,
    Address,
}

impl Kind {
    fn map(self) -> &'static str {
        match self {
            Kind::Email => "emails",
            Kind::Phone => "phones",
            Kind::Address => "addresses",
        }
    }

    fn type_name(self) -> &'static str {
        match self {
            Kind::Email => "EmailAddress",
            Kind::Phone => "Phone",
            Kind::Address => "Address",
        }
    }

    /// The first letter of the keys the app gives new entries.
    fn prefix(self) -> &'static str {
        match self {
            Kind::Email => "e",
            Kind::Phone => "p",
            Kind::Address => "a",
        }
    }

    /// What an entry says; empty when it says nothing the app can show.
    fn read(self, entry: &Value) -> String {
        match self {
            Kind::Email => text(&entry["address"]).to_string(),
            Kind::Phone => text(&entry["number"]).to_string(),
            Kind::Address => address_lines(entry),
        }
    }

    /// Makes `value` what an entry says. A postal address becomes free text: the parts it was
    /// made of would otherwise say something else.
    fn write(self, entry: &mut Map<String, Value>, value: &str) {
        match self {
            Kind::Email => entry.insert("address".into(), json!(value)),
            Kind::Phone => entry.insert("number".into(), json!(value)),
            Kind::Address => {
                for part in ["components", "isOrdered", "defaultSeparator"] {
                    entry.remove(part);
                }
                entry.insert("full".into(), json!(value))
            }
        };
    }
}

fn text(v: &Value) -> &str {
    v.as_str().map(str::trim).unwrap_or_default()
}

/// The entries of a map in the order they are shown: the preferred first (`pref` 1 is the most
/// preferred), then by key with a number in it by its value, so `e2` comes before `e10`.
fn entries(map: &Value) -> Vec<(&String, &Value)> {
    let mut list: Vec<_> = map.as_object().into_iter().flatten().collect();
    let pref = |entry: &Value| entry["pref"].as_u64().unwrap_or(u64::MAX);
    list.sort_by(|(ka, a), (kb, b)| pref(a).cmp(&pref(b)).then_with(|| key_order(ka, kb)));
    list
}

/// Keys by their text and then the number at their end; equal when that is no help, by text.
fn key_order(a: &str, b: &str) -> Ordering {
    let split = |key: &'_ str| {
        let at = key.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        (key[..at].to_string(), key[at..].parse::<u64>().ok())
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

/// Lines trimmed, blank ones dropped.
fn lines(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// "work" or "private" (what `contexts` says), else the entry's own label; empty when neither.
fn label_of(entry: &Value) -> String {
    match CONTEXTS.iter().find(|c| entry["contexts"][**c] == true) {
        Some(context) => context.to_string(),
        None => text(&entry["label"]).to_string(),
    }
}

/// Makes `label` what an entry is labeled: a context for "work" and "private", else its own
/// text. Other contexts of the entry stay.
fn set_label(entry: &mut Map<String, Value>, label: &str) {
    if let Some(Value::Object(contexts)) = entry.get_mut("contexts") {
        for context in CONTEXTS {
            contexts.remove(context);
        }
        if contexts.is_empty() {
            entry.remove("contexts");
        }
    }
    entry.remove("label");
    match label.to_lowercase().as_str() {
        "" => {}
        context @ ("work" | "private") => {
            let contexts = entry.entry("contexts").or_insert_with(|| json!({}));
            contexts[context] = json!(true);
        }
        _ => {
            entry.insert("label".into(), json!(label));
        }
    }
}

/// A postal address as lines: its `full` text, else its parts, the street first, then the
/// postcode and town, the region and the country, one line each.
fn address_lines(entry: &Value) -> String {
    let full = lines(entry["full"].as_str().unwrap_or_default());
    if !full.is_empty() {
        return full;
    }
    let parts: Vec<(&str, &str)> = entry["components"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| (text(&c["kind"]), text(&c["value"])))
        .filter(|(kind, value)| !value.is_empty() && *kind != "separator")
        .collect();
    let of = |kinds: &[&str]| parts.iter().filter(|(k, _)| kinds.contains(k)).map(|(_, v)| *v).collect::<Vec<_>>();
    let street: Vec<&str> = parts.iter().filter(|(k, _)| !TOWN_PARTS.contains(k)).map(|(_, v)| *v).collect();
    let town = [of(&["postcode"]), of(&["locality"])].concat();
    let lines = [street, town, of(&["region"]), of(&["country"])];
    lines.iter().filter(|l| !l.is_empty()).map(|l| l.join(" ")).collect::<Vec<_>>().join("\n")
}

fn read_list(card: &Value, kind: Kind) -> Vec<Labeled> {
    let one = |(_, entry): (&String, &Value)| {
        let value = kind.read(entry);
        (!value.is_empty()).then(|| Labeled { label: label_of(entry), value })
    };
    entries(&card[kind.map()]).into_iter().filter_map(one).collect()
}

/// The first entry of a map that has `field`: its key and text.
fn first_text<'a>(map: &'a Value, field: &str) -> Option<(&'a String, &'a str)> {
    entries(map).into_iter().map(|(key, entry)| (key, text(&entry[field]))).find(|(_, value)| !value.is_empty())
}

/// The first name component of a kind (given, surname), or nothing.
fn component<'a>(card: &'a Value, kind: &str) -> &'a str {
    let mut parts = card["name"]["components"].as_array().into_iter().flatten();
    parts.find(|c| text(&c["kind"]) == kind).map_or("", |c| text(&c["value"]))
}

/// The card's address book: the one it is in with the lowest id (a map of ids has no order).
fn first_book(card: &Value) -> String {
    entries(&card["addressBookIds"])
        .into_iter()
        .find(|(_, v)| **v == true)
        .map(|(id, _)| id.clone())
        .unwrap_or_default()
}

/// The name as it is shown: the card's full name, else its name components in order, else its
/// organization, first mail address or first phone number.
fn shown_name(card: &Value, contact: &Contact) -> String {
    let full = text(&card["name"]["full"]);
    if !full.is_empty() {
        return full.to_string();
    }
    let parts = card["name"]["components"].as_array().into_iter().flatten();
    let words: Vec<&str> = parts
        .filter(|c| text(&c["kind"]) != "separator")
        .map(|c| text(&c["value"]))
        .filter(|w| !w.is_empty())
        .collect();
    let fallbacks = [
        words.join(" "),
        contact.organization.clone(),
        contact.emails.first().map(|e| e.value.clone()).unwrap_or_default(),
        contact.phones.first().map(|p| p.value.clone()).unwrap_or_default(),
    ];
    fallbacks.into_iter().find(|w| !w.is_empty()).unwrap_or_else(|| NO_NAME.to_string())
}

/// What the app shows of a card.
pub fn contact_from(card: &Value) -> Contact {
    let mut contact = Contact {
        address_book_id: first_book(card),
        name: String::new(),
        given_name: component(card, "given").to_string(),
        family_name: component(card, "surname").to_string(),
        organization: first_text(&card[ORGANIZATION.map], ORGANIZATION.field).map_or(String::new(), |(_, v)| v.into()),
        emails: read_list(card, Kind::Email),
        phones: read_list(card, Kind::Phone),
        addresses: read_list(card, Kind::Address),
        notes: first_text(&card[NOTE.map], NOTE.field).map_or(String::new(), |(_, v)| v.into()),
    };
    contact.name = shown_name(card, &contact);
    contact
}

/// The address books of an `AddressBook/get` answer, by `sortOrder`. One with no rights stated
/// is read-only.
pub fn address_books_from(get: &Value) -> Vec<AddressBook> {
    let mut out: Vec<(i64, AddressBook)> = Vec::new();
    for b in get["list"].as_array().into_iter().flatten() {
        let Some(id) = b["id"].as_str() else { continue };
        out.push((
            b["sortOrder"].as_i64().unwrap_or(0),
            AddressBook {
                id: id.to_string(),
                name: b["name"].as_str().unwrap_or_default().to_string(),
                is_default: b["isDefault"] == true,
                may_write: b["myRights"]["mayWrite"] == true,
            },
        ));
    }
    out.sort_by_key(|(order, _)| *order);
    out.into_iter().map(|(_, book)| book).collect()
}

// ---- checking what the user typed ----

/// One "@" with something before and after it and no white space.
fn is_mail_address(address: &str) -> bool {
    match address.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.is_empty()
                && !domain.contains('@')
                && !address.chars().any(char::is_whitespace)
        }
        None => false,
    }
}

/// Trims what the user typed and drops the entries with no value.
fn tidy(list: &[Labeled], kind: Kind) -> Vec<Labeled> {
    let one = |l: &Labeled| {
        let value = if matches!(kind, Kind::Address) { lines(&l.value) } else { l.value.trim().to_string() };
        (!value.is_empty()).then(|| Labeled { label: l.label.trim().to_string(), value })
    };
    list.iter().filter_map(one).collect()
}

/// The contact as it will be written, or what is wrong with it. `current` is what the card says
/// now (for a change): a mail address that is already on it is not checked again, so a card
/// from another program does not become impossible to change. A name left out is made of the
/// given and family names.
pub fn validate(contact: &Contact, books: &[AddressBook], current: Option<&Contact>) -> Result<Contact> {
    let t = |s: &str| s.trim().to_string();
    let mut clean = Contact {
        address_book_id: t(&contact.address_book_id),
        name: t(&contact.name),
        given_name: t(&contact.given_name),
        family_name: t(&contact.family_name),
        organization: t(&contact.organization),
        emails: tidy(&contact.emails, Kind::Email),
        phones: tidy(&contact.phones, Kind::Phone),
        addresses: tidy(&contact.addresses, Kind::Address),
        notes: t(&contact.notes),
    };
    if clean.name.is_empty() {
        clean.name = [clean.given_name.as_str(), clean.family_name.as_str()]
            .into_iter()
            .filter(|n| !n.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    if clean.name.is_empty() && clean.emails.is_empty() {
        return Err(Error::invalid("Give the contact a name or a mail address."));
    }
    let known = |address: &str| current.is_some_and(|c| c.emails.iter().any(|e| e.value == address));
    if let Some(bad) = clean.emails.iter().find(|e| !is_mail_address(&e.value) && !known(&e.value)) {
        return Err(Error::invalid(format!(
            "\"{}\" is not a mail address: it needs one @ with something before and after it and no spaces.",
            bad.value
        )));
    }
    if clean.address_book_id.is_empty() {
        return Err(Error::invalid("Choose an address book."));
    }
    match books.iter().find(|b| b.id == clean.address_book_id) {
        None => Err(Error::invalid("This address book does not exist any more.")),
        Some(book) if !book.may_write => {
            Err(Error::invalid(format!("The address book \"{}\" is read-only.", book.name)))
        }
        Some(_) => Ok(clean),
    }
}

// ---- making a card ----

/// A key for a new entry that is in neither map.
fn fresh_key(prefix: &str, taken: &[&Map<String, Value>]) -> String {
    (1..).map(|n| format!("{prefix}{n}")).find(|key| taken.iter().all(|map| !map.contains_key(key))).unwrap_or_default()
}

fn new_entry(kind: Kind, item: &Labeled) -> Value {
    let mut entry = Map::new();
    entry.insert("@type".into(), json!(kind.type_name()));
    kind.write(&mut entry, &item.value);
    set_label(&mut entry, &item.label);
    Value::Object(entry)
}

/// The whole of a map of new entries, keyed `e1`, `e2`, ...
fn new_map(kind: Kind, list: &[Labeled]) -> Value {
    let mut map = Map::new();
    for item in list {
        map.insert(fresh_key(kind.prefix(), &[&map]), new_entry(kind, item));
    }
    Value::Object(map)
}

fn new_components(given: &str, family: &str) -> Vec<Value> {
    let part = |kind: &str, value: &str| json!({ "@type": "NameComponent", "kind": kind, "value": value });
    [("given", given), ("surname", family)].iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| part(k, v)).collect()
}

/// The `ContactCard` that makes a contact, which `validate` has passed.
pub fn create_object(contact: &Contact) -> Value {
    let mut card = Map::new();
    card.insert("@type".into(), json!("Card"));
    card.insert("version".into(), json!("1.0"));
    card.insert("addressBookIds".into(), json!({ contact.address_book_id.as_str(): true }));
    let components = new_components(&contact.given_name, &contact.family_name);
    if !contact.name.is_empty() || !components.is_empty() {
        let mut name = Map::new();
        name.insert("@type".into(), json!("Name"));
        if !contact.name.is_empty() {
            name.insert("full".into(), json!(contact.name));
        }
        if !components.is_empty() {
            name.insert("components".into(), json!(components));
            name.insert("isOrdered".into(), json!(true));
        }
        card.insert("name".into(), Value::Object(name));
    }
    if !contact.organization.is_empty() {
        card.insert("organizations".into(), json!({ "o1": { "@type": "Organization", "name": contact.organization } }));
    }
    if !contact.notes.is_empty() {
        card.insert("notes".into(), json!({ "n1": { "@type": "Note", "note": contact.notes } }));
    }
    for (kind, list) in
        [(Kind::Email, &contact.emails), (Kind::Phone, &contact.phones), (Kind::Address, &contact.addresses)]
    {
        if !list.is_empty() {
            card.insert(kind.map().into(), new_map(kind, list));
        }
    }
    Value::Object(card)
}

// ---- changing a card ----

/// The name components with the first given and surname ones set to the new values (taken out
/// when empty, put in when missing: the given name after a leading title, the family name after
/// the given ones). Every other component stays.
fn changed_components(existing: &Value, given: &str, family: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let (mut given_done, mut family_done) = (false, false);
    for part in existing.as_array().into_iter().flatten() {
        let (done, value) = match text(&part["kind"]) {
            "given" if !given_done => (&mut given_done, given),
            "surname" if !family_done => (&mut family_done, family),
            _ => {
                out.push(part.clone());
                continue;
            }
        };
        *done = true;
        if !value.is_empty() {
            let mut part = part.clone();
            part["value"] = json!(value);
            out.push(part);
        }
    }
    let new = new_components(given, family);
    if !given_done && !given.is_empty() {
        let leading = out.iter().take_while(|c| matches!(text(&c["kind"]), "title" | "separator")).count();
        out.insert(leading, new[0].clone());
    }
    if !family_done && !family.is_empty() {
        let after_given = out.iter().rposition(|c| matches!(text(&c["kind"]), "given" | "given2"));
        out.insert(after_given.map_or(out.len(), |i| i + 1), new[new.len() - 1].clone());
    }
    out
}

/// The name, as the form changed it (against `base`) and as the card does not say it already.
fn name_patch(card: &Value, now: &Contact, wanted: &Contact, base: &Contact, patch: &mut Map<String, Value>) {
    let full_changed = wanted.name != base.name && wanted.name != now.name;
    let parts = |c: &Contact| (c.given_name.clone(), c.family_name.clone());
    let parts_changed = parts(wanted) != parts(base) && parts(wanted) != parts(now);
    if !full_changed && !parts_changed {
        return;
    }
    let components = changed_components(&card["name"]["components"], &wanted.given_name, &wanted.family_name);
    if card["name"].is_object() {
        if full_changed {
            patch.insert("name/full".into(), if wanted.name.is_empty() { Value::Null } else { json!(wanted.name) });
        }
        if parts_changed {
            patch.insert("name/components".into(), if components.is_empty() { Value::Null } else { json!(components) });
        }
    } else {
        let mut name = Map::new();
        name.insert("@type".into(), json!("Name"));
        if !wanted.name.is_empty() {
            name.insert("full".into(), json!(wanted.name));
        }
        if !components.is_empty() {
            name.insert("components".into(), json!(components));
        }
        patch.insert("name".into(), Value::Object(name));
    }
}

/// A whole map as a patch value: `null` when it has nothing left.
fn map_value(map: Map<String, Value>) -> Value {
    if map.is_empty() {
        Value::Null
    } else {
        Value::Object(map)
    }
}

/// Sets the text of the first entry of a map (an organization's name, a note), makes one, or
/// removes it when `wanted` is empty. `None` when the form did not change it (`base`) or the
/// card says that already.
fn first_text_patch(card: &Value, of: &TextField, wanted: &str, base: &str) -> Option<Value> {
    let first = first_text(&card[of.map], of.field);
    if wanted == base || first.map_or("", |(_, v)| v) == wanted {
        return None;
    }
    let mut map = card[of.map].as_object().cloned().unwrap_or_default();
    match first {
        Some((key, _)) if wanted.is_empty() => {
            map.remove(key);
        }
        Some((key, _)) => {
            map.get_mut(key)?[of.field] = json!(wanted);
        }
        None => {
            let key = fresh_key(of.prefix, &[&map]);
            map.insert(key, json!({ "@type": of.type_name, of.field: wanted }));
        }
    }
    Some(map_value(map))
}

/// The values of `a` that `b` does not have as often, in the order of `a`.
fn difference(a: &[Labeled], b: &[Labeled]) -> Vec<Labeled> {
    let (mut rest, mut out) = (b.to_vec(), Vec::new());
    for x in a {
        match rest.iter().position(|y| y == x) {
            Some(i) => drop(rest.remove(i)),
            None => out.push(x.clone()),
        }
    }
    out
}

/// The map of a list of entries as it should be after the change, or `None` when nothing is to
/// be done. What the form changed is what differs from `base`: a value it no longer has is
/// removed from the card (when the card still has it), one it has anew is added, and a removed
/// and an added value pair off in order, the added one taking over the entry (and what the app
/// does not own in it) with its new value or label. Entries the card has that the form never
/// saw, and those the app cannot show, stay.
fn list_patch(card: &Value, kind: Kind, wanted: &[Labeled], base: &[Labeled]) -> Option<Value> {
    let (removed, added) = (difference(base, wanted), difference(wanted, base));
    let old_map = card[kind.map()].as_object().cloned().unwrap_or_default();
    let shown: Vec<(&String, Labeled)> = entries(&card[kind.map()])
        .into_iter()
        .map(|(key, e)| (key, Labeled { label: label_of(e), value: kind.read(e) }))
        .filter(|(_, l)| !l.value.is_empty())
        .collect();
    // Where each removed value is on the card now: not at all when somebody removed it already.
    let mut used = vec![false; shown.len()];
    let places: Vec<Option<usize>> = removed
        .iter()
        .map(|gone| {
            let i = (0..shown.len()).find(|i| !used[*i] && shown[*i].1 == *gone)?;
            used[i] = true;
            Some(i)
        })
        .collect();

    let mut map = old_map.clone();
    for (k, place) in places.iter().enumerate() {
        let Some(i) = *place else { continue };
        let (key, was) = &shown[i];
        let Some(item) = added.get(k) else {
            map.remove(key.as_str());
            continue;
        };
        let mut entry = old_map[key.as_str()].as_object().cloned().unwrap_or_default();
        if was.value != item.value {
            kind.write(&mut entry, &item.value);
        }
        if was.label != item.label {
            set_label(&mut entry, &item.label);
        }
        map.insert((*key).clone(), Value::Object(entry));
    }
    for (k, item) in added.iter().enumerate() {
        if places.get(k).is_none_or(Option::is_none) {
            map.insert(fresh_key(kind.prefix(), &[&old_map, &map]), new_entry(kind, item));
        }
    }
    (map != old_map).then(|| map_value(map))
}

/// A PatchObject that makes the card say what the user changed, and nothing else: of `wanted`
/// (which `validate` has passed) what differs from `base`, the contact as the form was opened
/// with (the name and its given and family parts, the first organization and note, the emails,
/// phones and addresses, the address book). Empty when there is nothing to change.
pub fn update_patch(card: &Value, wanted: &Contact, base: &Contact) -> Value {
    let now = contact_from(card);
    let mut patch = Map::new();
    name_patch(card, &now, wanted, base, &mut patch);
    for (field, change) in [
        (ORGANIZATION.map, first_text_patch(card, &ORGANIZATION, &wanted.organization, &base.organization)),
        (NOTE.map, first_text_patch(card, &NOTE, &wanted.notes, &base.notes)),
        ("emails", list_patch(card, Kind::Email, &wanted.emails, &base.emails)),
        ("phones", list_patch(card, Kind::Phone, &wanted.phones, &base.phones)),
        ("addresses", list_patch(card, Kind::Address, &wanted.addresses, &base.addresses)),
    ] {
        if let Some(value) = change {
            patch.insert(field.into(), value);
        }
    }
    if wanted.address_book_id != base.address_book_id && wanted.address_book_id != now.address_book_id {
        if !now.address_book_id.is_empty() {
            patch.insert(format!("addressBookIds/{}", pointer_segment(&now.address_book_id)), Value::Null);
        }
        patch.insert(format!("addressBookIds/{}", pointer_segment(&wanted.address_book_id)), json!(true));
    }
    Value::Object(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card as Stalwart 1.1.0 gave it back after it was made with everything the app shows
    /// and a few things it does not.
    fn rich() -> Value {
        json!({
            "@type": "Card", "version": "1.0", "kind": "individual", "id": "c1", "addressBookIds": { "b": true },
            "name": {
                "@type": "Name", "full": "Zoë Özdemir", "isOrdered": true,
                "components": [
                    { "@type": "NameComponent", "kind": "given", "value": "Zoë" },
                    { "@type": "NameComponent", "kind": "surname", "value": "Özdemir" }
                ]
            },
            "organizations": { "o1": { "@type": "Organization", "name": "Acme" } },
            "emails": {
                "e1": { "@type": "EmailAddress", "address": "zoe@x.test", "contexts": { "work": true }, "pref": 1 },
                "e2": { "@type": "EmailAddress", "address": "zoe2@x.test", "label": "club" }
            },
            "phones": {
                "p1": { "@type": "Phone", "number": "+49 30 1234", "contexts": { "private": true }, "features": { "mobile": true } },
                "p2": { "@type": "Phone", "number": "  " }
            },
            "addresses": {
                "a1": {
                    "@type": "Address", "contexts": { "work": true }, "isOrdered": true,
                    "components": [
                        { "@type": "AddressComponent", "kind": "name", "value": "Main St 1" },
                        { "@type": "AddressComponent", "kind": "locality", "value": "Berlin" }
                    ]
                }
            },
            "notes": { "n1": { "@type": "Note", "note": "hello" } },
            "nicknames": { "k1": { "@type": "Nickname", "name": "Z" } }
        })
    }

    /// The patch of a form opened on this very card.
    fn patch_of(card: &Value, wanted: &Contact) -> Value {
        update_patch(card, wanted, &contact_from(card))
    }

    fn labeled(label: &str, value: &str) -> Labeled {
        Labeled { label: label.into(), value: value.into() }
    }

    fn book(id: &str, may_write: bool) -> AddressBook {
        AddressBook { id: id.into(), name: format!("Book {id}"), is_default: id == "b", may_write }
    }

    #[test]
    fn a_full_card_is_read_with_labels_and_joined_address_lines() {
        let c = contact_from(&rich());
        assert_eq!(
            (c.address_book_id.as_str(), c.name.as_str(), c.given_name.as_str(), c.family_name.as_str()),
            ("b", "Zoë Özdemir", "Zoë", "Özdemir")
        );
        assert_eq!((c.organization.as_str(), c.notes.as_str()), ("Acme", "hello"));
        assert_eq!(c.emails, [labeled("work", "zoe@x.test"), labeled("club", "zoe2@x.test")]);
        assert_eq!(c.phones, [labeled("private", "+49 30 1234")], "a phone with no number is not shown");
        assert_eq!(c.addresses, [labeled("work", "Main St 1\nBerlin")]);
    }

    #[test]
    fn a_card_with_only_an_email_is_named_by_it() {
        let c =
            contact_from(&json!({ "emails": { "a": { "address": " ann@x.test " } }, "addressBookIds": { "b": true } }));
        assert_eq!((c.name.as_str(), c.given_name.as_str(), c.organization.as_str()), ("ann@x.test", "", ""));
        assert_eq!(c.emails, [labeled("", "ann@x.test")]);
        // What Stalwart makes of a card with nothing: an empty full name.
        let empty = contact_from(&json!({ "name": { "full": "" }, "addressBookIds": { "b": true } }));
        assert_eq!(empty.name, NO_NAME);
        assert_eq!(contact_from(&json!({ "name": {}, "phones": { "p": { "number": "0170 1" } } })).name, "0170 1");
    }

    #[test]
    fn a_company_card_is_named_by_its_organization() {
        let card = json!({
            "kind": "org", "addressBookIds": { "b": true }, "name": { "full": "" },
            "organizations": { "o": { "name": "Kweb Ltd" } }, "emails": { "a": { "address": "info@kweb.test" } }
        });
        let c = contact_from(&card);
        assert_eq!((c.name.as_str(), c.organization.as_str()), ("Kweb Ltd", "Kweb Ltd"));
    }

    #[test]
    fn name_components_make_the_name_when_there_is_no_full_name() {
        let card = json!({ "name": { "components": [
            { "kind": "title", "value": "Dr." }, { "kind": "given", "value": "Maria" },
            { "kind": "given2", "value": "Luisa" }, { "kind": "separator", "value": ", " },
            { "kind": "surname", "value": "Keller" }, { "kind": "surname2", "value": "" }
        ] } });
        let c = contact_from(&card);
        assert_eq!(
            (c.name.as_str(), c.given_name.as_str(), c.family_name.as_str()),
            ("Dr. Maria Luisa Keller", "Maria", "Keller")
        );
    }

    #[test]
    fn several_names_and_entries_show_the_first_of_each_and_all_labeled_values() {
        let card = json!({
            "addressBookIds": { "b": true, "w": true },
            "name": { "full": "Ann Lee", "components": [
                { "kind": "given", "value": "Ann" }, { "kind": "given", "value": "Marie" },
                { "kind": "surname", "value": "Lee" }
            ] },
            "organizations": { "o1": { "name": "" }, "o2": { "name": "First" }, "o3": { "name": "Second" } },
            "notes": { "n1": { "note": "one" }, "n2": { "note": "two" } },
            "emails": { "e1": { "address": "a@x" }, "e2": { "address": "b@x", "contexts": { "private": true, "work": true } } }
        });
        let c = contact_from(&card);
        assert_eq!((c.given_name.as_str(), c.organization.as_str(), c.notes.as_str()), ("Ann", "First", "one"));
        assert_eq!(c.emails, [labeled("", "a@x"), labeled("work", "b@x")], "work wins over private");
    }

    #[test]
    fn non_latin_names_are_kept_as_they_are() {
        let jp = json!({ "name": { "components": [
            { "kind": "surname", "value": "山田" }, { "kind": "given", "value": "太郎" }
        ], "isOrdered": true } });
        let c = contact_from(&jp);
        assert_eq!((c.name.as_str(), c.given_name.as_str(), c.family_name.as_str()), ("山田 太郎", "太郎", "山田"));
        let ru =
            contact_from(&json!({ "name": { "full": "Дмитрий Ёж" }, "emails": { "a": { "address": "д@пример.рф" } } }));
        assert_eq!((ru.name.as_str(), ru.emails[0].value.as_str()), ("Дмитрий Ёж", "д@пример.рф"));
        assert!(is_mail_address("д@пример.рф"));
    }

    #[test]
    fn an_address_without_full_text_is_street_then_town_region_country() {
        let entry = json!({ "components": [
            { "kind": "country", "value": "Germany" }, { "kind": "number", "value": "1" },
            { "kind": "postcode", "value": "10115" }, { "kind": "name", "value": "Main St" },
            { "kind": "locality", "value": "Berlin" }, { "kind": "region", "value": "BE" },
            { "kind": "separator", "value": ", " }
        ] });
        assert_eq!(address_lines(&entry), "1 Main St\n10115 Berlin\nBE\nGermany");
        assert_eq!(address_lines(&json!({ "full": " 1 Road \n\n Town " })), "1 Road\nTown");
        assert_eq!(address_lines(&json!({})), "");
    }

    #[test]
    fn the_books_come_by_sort_order_and_need_the_right_to_write() {
        let get = json!({ "list": [
            { "id": "b", "name": "Mine", "sortOrder": 0, "isDefault": true, "myRights": { "mayRead": true, "mayWrite": true } },
            { "id": "s", "name": "Shared", "sortOrder": -1, "myRights": { "mayRead": true, "mayWrite": false } },
            { "id": "n", "name": "No rights" },
            { "name": "no id" }
        ] });
        let books = address_books_from(&get);
        let flags = |b: &AddressBook| (b.id.clone(), b.is_default, b.may_write);
        assert_eq!(
            books.iter().map(flags).collect::<Vec<_>>(),
            [("s".into(), false, false), ("b".into(), true, true), ("n".into(), false, false)]
        );
    }

    // ---- validating ----

    fn typed() -> Contact {
        Contact {
            address_book_id: "b".into(),
            name: " Ann Lee ".into(),
            given_name: String::new(),
            family_name: String::new(),
            organization: String::new(),
            emails: vec![labeled(" work ", " ann@x.test "), labeled("", "  ")],
            phones: vec![labeled("", ""), labeled("", " 0170 1 ")],
            addresses: vec![labeled("", " 1 Road \n\n Town "), labeled("home", "\n")],
            notes: "  \n".into(),
        }
    }

    fn refused(contact: &Contact, current: Option<&Contact>) -> String {
        match validate(contact, &[book("b", true), book("r", false)], current) {
            Err(Error::Invalid { msg }) => msg,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn what_is_typed_is_trimmed_and_blank_entries_are_dropped_not_refused() {
        let clean = validate(&typed(), &[book("b", true)], None).unwrap();
        assert_eq!(clean.name, "Ann Lee");
        assert_eq!(clean.emails, [labeled("work", "ann@x.test")]);
        assert_eq!(clean.phones, [labeled("", "0170 1")]);
        assert_eq!(clean.addresses, [labeled("", "1 Road\nTown")]);
        assert_eq!(clean.notes, "");
    }

    #[test]
    fn a_contact_needs_a_name_or_a_mail_address() {
        let mut c = typed();
        c.name = "  ".into();
        assert!(validate(&c, &[book("b", true)], None).is_ok(), "the mail address will do");
        c.emails.clear();
        assert_eq!(refused(&c, None), "Give the contact a name or a mail address.");
        c.given_name = "Ann".into();
        c.family_name = "Lee".into();
        assert_eq!(validate(&c, &[book("b", true)], None).unwrap().name, "Ann Lee", "made of the parts");
    }

    #[test]
    fn a_mail_address_needs_one_at_sign_with_text_on_both_sides_and_no_spaces() {
        for good in ["a@b", "a.b+c@d.example", "x@y.z"] {
            assert!(is_mail_address(good), "{good}");
        }
        assert!(!is_mail_address(""), "a blank one is dropped before it is checked");
        for bad in ["ab", "@b", "a@", "a@b@c", "a b@c", "a@b c", "a@\tb"] {
            assert!(!is_mail_address(bad), "{bad:?}");
            let mut c = typed();
            c.emails = vec![labeled("", bad)];
            assert!(refused(&c, None).contains("is not a mail address"), "{bad:?}");
        }
        // One the card has already is let through, so a card from elsewhere can still be changed.
        let mut c = typed();
        c.emails = vec![labeled("", "junk")];
        let current = Contact { emails: vec![labeled("", "junk")], ..typed() };
        assert!(validate(&c, &[book("b", true)], Some(&current)).is_ok());
        assert!(validate(&c, &[book("b", true)], None).is_err());
    }

    #[test]
    fn the_address_book_must_be_known_and_writable() {
        let mut c = typed();
        c.address_book_id = " ".into();
        assert_eq!(refused(&c, None), "Choose an address book.");
        c.address_book_id = "zz".into();
        assert_eq!(refused(&c, None), "This address book does not exist any more.");
        c.address_book_id = "r".into();
        assert_eq!(refused(&c, None), "The address book \"Book r\" is read-only.");
    }

    // ---- making ----

    #[test]
    fn a_new_contact_is_made_with_the_exact_card() {
        let contact = Contact {
            address_book_id: "b".into(),
            name: "Zoë Özdemir".into(),
            given_name: "Zoë".into(),
            family_name: "Özdemir".into(),
            organization: "Acme".into(),
            emails: vec![labeled("work", "zoe@x.test"), labeled("club", "z2@x.test"), labeled("", "z3@x.test")],
            phones: vec![labeled("private", "+49 30 1234")],
            addresses: vec![labeled("Work", "Main St 1\nBerlin")],
            notes: "hello".into(),
        };
        assert_eq!(
            create_object(&contact),
            json!({
                "@type": "Card", "version": "1.0", "addressBookIds": { "b": true },
                "name": {
                    "@type": "Name", "full": "Zoë Özdemir", "isOrdered": true,
                    "components": [
                        { "@type": "NameComponent", "kind": "given", "value": "Zoë" },
                        { "@type": "NameComponent", "kind": "surname", "value": "Özdemir" }
                    ]
                },
                "organizations": { "o1": { "@type": "Organization", "name": "Acme" } },
                "notes": { "n1": { "@type": "Note", "note": "hello" } },
                "emails": {
                    "e1": { "@type": "EmailAddress", "address": "zoe@x.test", "contexts": { "work": true } },
                    "e2": { "@type": "EmailAddress", "address": "z2@x.test", "label": "club" },
                    "e3": { "@type": "EmailAddress", "address": "z3@x.test" }
                },
                "phones": { "p1": { "@type": "Phone", "number": "+49 30 1234", "contexts": { "private": true } } },
                "addresses": { "a1": { "@type": "Address", "full": "Main St 1\nBerlin", "contexts": { "work": true } } }
            })
        );
        let bare = Contact {
            name: String::new(),
            given_name: String::new(),
            family_name: String::new(),
            organization: String::new(),
            emails: vec![labeled("", "a@x")],
            phones: vec![],
            addresses: vec![],
            notes: String::new(),
            ..contact
        };
        assert_eq!(
            create_object(&bare),
            json!({ "@type": "Card", "version": "1.0", "addressBookIds": { "b": true },
                    "emails": { "e1": { "@type": "EmailAddress", "address": "a@x" } } })
        );
    }

    // ---- changing ----

    #[test]
    fn a_card_read_and_saved_unchanged_has_an_empty_patch() {
        for card in [
            rich(),
            json!({ "emails": { "a": { "address": "ann@x.test" } }, "addressBookIds": { "b": true } }),
            json!({ "name": { "full": "" }, "addressBookIds": { "b": true } }),
            json!({ "name": { "components": [{ "kind": "surname", "value": "山田" }] }, "addressBookIds": { "b": true },
                    "organizations": { "o": { "name": "Kweb" } }, "phones": { "p": { "uri": "tel:+1" } } }),
        ] {
            let contact = contact_from(&card);
            assert_eq!(patch_of(&card, &contact), json!({}), "{card}");
            // Through the check the app puts it through as well.
            let clean = validate(&contact, &[book("b", true)], Some(&contact)).unwrap();
            assert_eq!(patch_of(&card, &clean), json!({}), "{card}");
        }
    }

    #[test]
    fn a_change_patches_only_what_the_app_owns_and_keeps_the_rest() {
        let card = rich();
        let mut c = contact_from(&card);
        c.name = "Zoe Z".into();
        c.given_name = "Zoe".into();
        c.organization = "Acme 2".into();
        c.notes = "bye".into();
        c.phones[0].value = "+49 30 9999".into();
        c.emails[1].label = "work".into();
        c.emails.push(labeled("", "new@x.test"));
        assert_eq!(
            patch_of(&card, &c),
            json!({
                "name/full": "Zoe Z",
                "name/components": [
                    { "@type": "NameComponent", "kind": "given", "value": "Zoe" },
                    { "@type": "NameComponent", "kind": "surname", "value": "Özdemir" }
                ],
                "organizations": { "o1": { "@type": "Organization", "name": "Acme 2" } },
                "notes": { "n1": { "@type": "Note", "note": "bye" } },
                "emails": {
                    "e1": { "@type": "EmailAddress", "address": "zoe@x.test", "contexts": { "work": true }, "pref": 1 },
                    "e2": { "@type": "EmailAddress", "address": "zoe2@x.test", "contexts": { "work": true } },
                    "e3": { "@type": "EmailAddress", "address": "new@x.test" }
                },
                "phones": {
                    "p1": { "@type": "Phone", "number": "+49 30 9999", "contexts": { "private": true }, "features": { "mobile": true } },
                    "p2": { "@type": "Phone", "number": "  " }
                }
            }),
            "nothing about the addresses, the nickname, the kind or the version"
        );
    }

    #[test]
    fn entries_that_did_not_change_keep_their_ids_when_another_is_removed_or_added() {
        let card = rich();
        let mut c = contact_from(&card);
        c.emails.remove(0);
        let patch = patch_of(&card, &c);
        assert_eq!(
            patch["emails"],
            json!({ "e2": { "@type": "EmailAddress", "address": "zoe2@x.test", "label": "club" } }),
            "e2 is kept as it was, e1 is gone"
        );
        c.emails.insert(0, labeled("", "first@x.test"));
        // The new first value takes over the entry the list lost (a rewrite, not a delete and a new id).
        let patch = patch_of(&card, &c);
        assert_eq!(patch["emails"]["e2"]["address"], "zoe2@x.test");
        assert_eq!(patch["emails"]["e1"], json!({ "@type": "EmailAddress", "address": "first@x.test", "pref": 1 }));
        // All gone: the map goes.
        c.emails.clear();
        c.phones.clear();
        let patch = patch_of(&card, &c);
        assert_eq!(patch["emails"], Value::Null);
        assert_eq!(
            patch["phones"],
            json!({ "p2": { "@type": "Phone", "number": "  " } }),
            "the entry that showed nothing stays"
        );
    }

    #[test]
    fn a_changed_address_becomes_free_text_and_keeps_its_context() {
        let card = rich();
        let mut c = contact_from(&card);
        c.addresses[0].value = "Side St 2\nHamburg".into();
        assert_eq!(
            patch_of(&card, &c)["addresses"],
            json!({ "a1": { "@type": "Address", "contexts": { "work": true }, "full": "Side St 2\nHamburg" } }),
            "the parts it was made of are gone, so they cannot say something else"
        );
    }

    #[test]
    fn a_card_with_nothing_gets_what_it_lacks() {
        let card = json!({ "@type": "Card", "id": "c", "addressBookIds": { "b": true }, "name": { "full": "" } });
        let mut c = contact_from(&card);
        c.name = "Ann".into();
        c.given_name = "Ann".into();
        c.family_name = "Lee".into();
        c.organization = "Acme".into();
        c.notes = "n".into();
        c.emails = vec![labeled("", "a@x")];
        let patch = patch_of(&card, &c);
        assert_eq!(patch["name/full"], "Ann");
        assert_eq!(
            patch["name/components"],
            json!([{ "@type": "NameComponent", "kind": "given", "value": "Ann" }, { "@type": "NameComponent", "kind": "surname", "value": "Lee" }])
        );
        assert_eq!(patch["organizations"], json!({ "o1": { "@type": "Organization", "name": "Acme" } }));
        assert_eq!(patch["notes"], json!({ "n1": { "@type": "Note", "note": "n" } }));
        assert_eq!(patch["emails"], json!({ "e1": { "@type": "EmailAddress", "address": "a@x" } }));
        // A card with no name object at all gets one.
        let bare = json!({ "addressBookIds": { "b": true } });
        let mut c = contact_from(&bare);
        c.name = "Ann".into();
        assert_eq!(patch_of(&bare, &c), json!({ "name": { "@type": "Name", "full": "Ann" } }));
    }

    #[test]
    fn the_name_parts_are_edited_in_place_and_other_components_stay() {
        let card = json!({ "addressBookIds": { "b": true }, "name": { "full": "Dr. Ann Lee", "components": [
            { "kind": "title", "value": "Dr." }, { "kind": "given", "value": "Ann" },
            { "kind": "given2", "value": "Marie" }, { "kind": "surname", "value": "Lee" }
        ] } });
        let mut c = contact_from(&card);
        c.given_name = "Anna".into();
        c.family_name = String::new();
        assert_eq!(
            patch_of(&card, &c),
            json!({ "name/components": [
                { "kind": "title", "value": "Dr." }, { "kind": "given", "value": "Anna" }, { "kind": "given2", "value": "Marie" }
            ] }),
            "the full name is not touched when it was not edited"
        );
        // A family name with no surname component goes after the given names.
        let card = json!({ "addressBookIds": { "b": true }, "name": { "components": [
            { "kind": "given", "value": "Ann" }, { "kind": "given2", "value": "M" }, { "kind": "suffix", "value": "Jr" }
        ] } });
        let mut c = contact_from(&card);
        c.family_name = "Lee".into();
        let patch = patch_of(&card, &c);
        let kinds: Vec<&str> =
            patch["name/components"].as_array().unwrap().iter().map(|p| p["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["given", "given2", "surname", "suffix"]);
        assert!(patch.get("name/full").is_none());
    }

    #[test]
    fn a_blank_name_removes_the_full_name_and_the_book_is_moved_by_key() {
        let card = rich();
        let mut c = contact_from(&card);
        c.name = String::new();
        c.address_book_id = "w/x".into();
        let patch = patch_of(&card, &c);
        assert_eq!(patch["name/full"], Value::Null);
        assert_eq!(patch["addressBookIds/b"], Value::Null);
        assert_eq!(patch["addressBookIds/w~1x"], json!(true));
    }

    #[test]
    fn a_given_name_that_is_new_goes_after_a_leading_title() {
        let card = json!({ "addressBookIds": { "b": true }, "name": { "isOrdered": true, "components": [
            { "kind": "title", "value": "Dr." }, { "kind": "surname", "value": "Lee" }
        ] } });
        let mut c = contact_from(&card);
        c.given_name = "Ann".into();
        let patch = patch_of(&card, &c);
        let kinds: Vec<&str> =
            patch["name/components"].as_array().unwrap().iter().map(|p| p["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["title", "given", "surname"]);
    }

    #[test]
    fn entries_come_by_preference_then_by_key_with_numbers_counted() {
        let card = json!({ "addressBookIds": { "z": true, "b": true }, "emails": {
            "work": { "address": "old@x" }, "zhome": { "address": "main@x", "pref": 1 },
            "e10": { "address": "ten@x" }, "e2": { "address": "two@x" }, "e1": { "address": "one@x", "pref": 2 }
        } });
        let c = contact_from(&card);
        let addresses: Vec<&str> = c.emails.iter().map(|e| e.value.as_str()).collect();
        assert_eq!(addresses, ["main@x", "one@x", "two@x", "ten@x", "old@x"]);
        assert_eq!(c.address_book_id, "b", "the lowest id of the books it is in");
        assert_eq!(c.name, "main@x");
    }

    #[test]
    fn only_what_differs_from_the_base_is_written_onto_the_card_as_it_is_now() {
        // The form was opened on a card with one address; since then two entries were added.
        let base = contact_from(&json!({ "addressBookIds": { "b": true }, "name": { "full": "Ann" },
            "emails": { "e1": { "address": "a@x" } }, "notes": { "n": { "note": "old" } } }));
        let now = json!({ "addressBookIds": { "b": true }, "name": { "full": "Ann L." },
            "emails": { "e1": { "address": "a@x" }, "e2": { "address": "b@x" } }, "notes": { "n": { "note": "new" } } });
        // Untouched form: nothing at all, whatever the card says now.
        assert_eq!(update_patch(&now, &base, &base), json!({}));
        // The user adds an address and edits nothing else.
        let mut wanted = base.clone();
        wanted.emails.push(labeled("", "c@x"));
        let patch = update_patch(&now, &wanted, &base);
        assert_eq!(patch.as_object().unwrap().keys().collect::<Vec<_>>(), ["emails"]);
        assert_eq!(patch["emails"]["e1"]["address"], "a@x");
        assert_eq!(patch["emails"]["e2"]["address"], "b@x");
        assert_eq!(patch["emails"]["e3"]["address"], "c@x");
        // The user removes the one address they saw: the one added elsewhere stays.
        wanted.emails.clear();
        assert_eq!(update_patch(&now, &wanted, &base)["emails"], json!({ "e2": { "address": "b@x" } }));
        // An edited field is written whatever the card says now.
        wanted.notes = "mine".into();
        assert_eq!(update_patch(&now, &wanted, &base)["notes"], json!({ "n": { "note": "mine" } }));
    }
}
