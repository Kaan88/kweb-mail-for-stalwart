//! Address books and contact cards over plain JMAP requests (jmap-client has no contacts
//! support). The JSContact side, and what Stalwart 1.1.0 does that the drafts do not say, is in
//! [`super::jscontact`].

use serde_json::{json, Value};

use super::{
    errors::is_state_mismatch,
    jscontact,
    send::{created_id, destroyed, response, updated, CREATE_ID},
    Connection,
};
use crate::{
    contacts::Entry,
    model::{AddressBook, Contact},
    Error, Result,
};

const CONTACTS_CAPABILITY: &str = "urn:ietf:params:jmap:contacts";
const NO_SUCH_CONTACT: &str = "the contact no longer exists on the server";
/// Where a listing is given up, for a server that never ends a listing.
const MAX_CARDS: usize = 50_000;
/// What the list and the suggestions need of a card. Without it every picture and other media
/// of every card would be downloaded with each listing.
const LIST_PROPERTIES: [&str; 8] =
    ["id", "addressBookIds", "name", "organizations", "emails", "phones", "addresses", "notes"];

/// Whether `e` says that the contact is not on the server any more.
pub fn is_no_such_contact(e: &Error) -> bool {
    matches!(e, Error::Invalid { msg } if msg == NO_SUCH_CONTACT)
}

impl Connection {
    fn contacts_account_id(&self) -> String {
        self.account_for(CONTACTS_CAPABILITY)
    }

    async fn contacts_call(&self, calls: &[(&str, Value)]) -> Result<Vec<(String, Value)>> {
        self.call(&[CONTACTS_CAPABILITY], calls).await
    }

    /// The user's address books, in the server's order (by `sortOrder`). A server without
    /// contacts has none.
    pub async fn address_books(&self) -> Result<Vec<AddressBook>> {
        if !self.has_capability(CONTACTS_CAPABILITY) {
            return Ok(Vec::new());
        }
        let args = json!({ "accountId": self.contacts_account_id() });
        let responses = self.contacts_call(&[("AddressBook/get", args)]).await?;
        Ok(jscontact::address_books_from(response(&responses, "AddressBook/get")?))
    }

    /// Every card of the account, oldest first. Each page is one request: the ids and, by
    /// reference, the cards, without their media. The next page starts after the ids the server
    /// gave, not after the limit asked for (a server may give fewer), and the listing ends at an
    /// empty page or at the total the server counted. A server without contacts has none.
    pub async fn contact_cards(&self) -> Result<Vec<Entry>> {
        if !self.has_capability(CONTACTS_CAPABILITY) {
            return Ok(Vec::new());
        }
        let (account, limit) = (self.contacts_account_id(), self.get_batch());
        let (mut out, mut position) = (Vec::new(), 0_usize);
        while position < MAX_CARDS {
            let query = json!({
                "accountId": account, "position": position, "limit": limit, "calculateTotal": true,
                "sort": [{ "property": "created" }],
            });
            let get = json!({
                "accountId": account, "properties": LIST_PROPERTIES,
                "#ids": { "resultOf": "c0", "name": "ContactCard/query", "path": "/ids" },
            });
            let responses = self.contacts_call(&[("ContactCard/query", query), ("ContactCard/get", get)]).await?;
            let answer = response(&responses, "ContactCard/query")?;
            let found = answer["ids"].as_array().map_or(0, Vec::len);
            let cards = response(&responses, "ContactCard/get")?["list"].as_array().into_iter().flatten();
            out.extend(cards.filter_map(entry_from));
            position += found;
            if found == 0 || answer["total"].as_u64().is_some_and(|total| position as u64 >= total) {
                break;
            }
        }
        Ok(out)
    }

    /// One card with the state of the cards it was read at. One the server does not have is
    /// `Error::Invalid`.
    async fn card_get(&self, id: &str) -> Result<(Value, Option<String>)> {
        let args = json!({ "accountId": self.contacts_account_id(), "ids": [id] });
        let responses = self.contacts_call(&[("ContactCard/get", args)]).await?;
        let answer = response(&responses, "ContactCard/get")?;
        let card = answer["list"].get(0).cloned().ok_or_else(|| Error::invalid(NO_SUCH_CONTACT))?;
        Ok((card, answer["state"].as_str().map(str::to_string)))
    }

    /// One contact, as the server has it now. One it does not have is `Error::Invalid`.
    pub async fn contact_card(&self, id: &str) -> Result<Entry> {
        let (card, _) = self.card_get(id).await?;
        entry_from(&card).ok_or_else(|| Error::invalid(NO_SUCH_CONTACT))
    }

    /// Creates a contact (`id` is `None`) or changes one. Returns its id. Only what the app owns
    /// is written, and of that only what differs from `base`, the contact as the form was
    /// opened with (`None`: as the card is now); what was changed elsewhere since stays. A
    /// change that changes nothing sends nothing.
    pub async fn save_contact(&self, id: Option<&str>, contact: &Contact, base: Option<&Contact>) -> Result<String> {
        let books = self.address_books().await?;
        let Some(id) = id else {
            let clean = jscontact::validate(contact, &books, None)?;
            let args = json!({
                "accountId": self.contacts_account_id(),
                "create": { CREATE_ID: jscontact::create_object(&clean) },
            });
            let responses = self.contacts_call(&[("ContactCard/set", args)]).await?;
            return created_id(response(&responses, "ContactCard/set")?);
        };
        self.change_card(id, |card| {
            let now = jscontact::contact_from(card);
            let clean = jscontact::validate(contact, &books, Some(base.unwrap_or(&now)))?;
            Ok(jscontact::update_patch(card, &clean, base.unwrap_or(&now)))
        })
        .await?;
        Ok(id.to_string())
    }

    /// Deletes a contact. One that is already gone is as good as deleted.
    pub async fn delete_contact(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.contacts_account_id(), "destroy": [id] });
        let responses = self.contacts_call(&[("ContactCard/set", args)]).await?;
        destroyed(response(&responses, "ContactCard/set")?, id)
    }

    /// Reads the card, builds a patch from it and sends it as of the state it was read at. A
    /// card someone else changed in between is read again and the patch rebuilt, once. An empty
    /// patch sends nothing.
    async fn change_card(&self, id: &str, build: impl Fn(&Value) -> Result<Value>) -> Result<()> {
        for _ in 0..2 {
            let (card, state) = self.card_get(id).await?;
            let patch = build(&card)?;
            if patch.as_object().is_some_and(|fields| fields.is_empty()) {
                return Ok(());
            }
            let mut args = json!({ "accountId": self.contacts_account_id(), "update": { id: patch } });
            if let Some(state) = state {
                args["ifInState"] = json!(state);
            }
            match self.contacts_call(&[("ContactCard/set", args)]).await {
                Err(e) if is_state_mismatch(&e) => continue,
                Err(e) => return Err(e),
                Ok(responses) => return updated(response(&responses, "ContactCard/set")?, id, NO_SUCH_CONTACT),
            }
        }
        Err(Error::temporary("the contact was changed elsewhere while it was being saved; try again"))
    }
}

/// A card of a `ContactCard/get` answer as a cache entry; `None` when it has no id.
fn entry_from(card: &Value) -> Option<Entry> {
    Some(Entry::new(card["id"].as_str()?.to_string(), jscontact::contact_from(card)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_without_an_id_is_not_an_entry() {
        let card = json!({ "id": "c1", "name": { "full": "Ann" }, "addressBookIds": { "b": true } });
        let entry = entry_from(&card).unwrap();
        assert_eq!(
            (entry.id.as_str(), entry.row.name.as_str(), entry.contact.address_book_id.as_str()),
            ("c1", "Ann", "b")
        );
        assert!(entry_from(&json!({ "name": { "full": "Ann" } })).is_none());
    }
}
