//! The server's domains over `x:Domain` and their DKIM keys over `x:DkimSignature` (see
//! `dev/notes/stalwart-admin-more.md`). What matters here:
//!
//! - `dnsZoneFile` is made by the server (read only): the records the administrator has to
//!   publish, one per line, a long one over several lines in parentheses.
//! - A new domain gets DKIM keys by itself (`dkimManagement` is `Automatic`), and the server
//!   refuses to delete a domain that anything still points at (`objectIsLinked`, with the
//!   linked objects named): its own DKIM keys count, so those are removed first, and only when
//!   they are all that is left.
//! - The domain name is made lower case by the server; a bad one is `invalidPatch`; a name that
//!   exists is `primaryKeyViolation`.
//! - A DKIM key is read without its private key and without its public key (the zone file has
//!   the public part). Keys are not made, rotated or deleted here.
//! - Deleting a domain is guarded here: a domain that an account still uses (as its address or
//!   as the domain of an alias) is refused. A key that may not read the accounts cannot check,
//!   so it cannot delete either.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{set_refusal, text, time, LIST_MAX};
use crate::{
    ffi::{DkimKey, DomainDetail, DomainRow},
    jmap::{
        send::{created_id, destroyed, response, updated, CREATE_ID},
        Connection,
    },
    Error, Result,
};

const GONE: &str = "that domain is no longer on the server";
const NAME_MAX: usize = 253;
const LABEL_MAX: usize = 63;
const ROW_PROPERTIES: [&str; 3] = ["name", "enable", "aliases"];
const DETAIL_PROPERTIES: [&str; 6] = ["name", "enable", "aliases", "dnsZoneFile", "dkimManagement", "dnsManagement"];
const KEY_PROPERTIES: [&str; 5] = ["@type", "selector", "stage", "nextTransitionAt", "createdAt"];

/// The domain name as the app lets an administrator add it: ASCII letters, digits, hyphens and
/// dots (an international name is typed in its `xn--` form), lower case, labels of at most 63
/// characters that do not start or end with a hyphen.
pub(crate) fn clean_name(name: &str) -> Result<String> {
    let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
    let bad = || Error::invalid("a domain name has letters, digits, hyphens and dots only, like example.org");
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(bad());
    }
    for label in name.split('.') {
        let ok = !label.is_empty()
            && label.len() <= LABEL_MAX
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !ok {
            return Err(bad());
        }
    }
    Ok(name)
}

/// A set of names as the server sends it (an object with `true` values) or as a list.
fn names(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = match v {
        Value::Object(map) => {
            map.iter().filter(|(_, on)| !matches!(on, Value::Bool(false))).map(|(k, _)| k.clone()).collect()
        }
        Value::Array(list) => list.iter().filter_map(|s| s.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    out.sort();
    out
}

fn domain_row(d: &Value) -> Option<DomainRow> {
    Some(DomainRow {
        id: d["id"].as_str()?.to_string(),
        name: text(d, "name"),
        enabled: !matches!(d["enable"], Value::Bool(false)),
        aliases: names(&d["aliases"]),
    })
}

/// The key's algorithm in words.
fn algorithm(kind: &str) -> String {
    match kind {
        "Dkim1Ed25519Sha256" => "Ed25519 (SHA-256)",
        "Dkim1RsaSha256" => "RSA (SHA-256)",
        "Dkim2Ed25519Sha256" => "DKIM2 Ed25519 (SHA-256)",
        "Dkim2RsaSha256" => "DKIM2 RSA (SHA-256)",
        "Dkim2MlDsa44" => "DKIM2 ML-DSA-44",
        "" => "",
        other => other,
    }
    .to_string()
}

fn dkim_key(k: &Value) -> Option<DkimKey> {
    Some(DkimKey {
        id: k["id"].as_str()?.to_string(),
        selector: text(k, "selector"),
        algorithm: algorithm(k["@type"].as_str().unwrap_or_default()),
        stage: text(k, "stage"),
        next_change_at: time(&k["nextTransitionAt"]),
        created_at: time(&k["createdAt"]),
    })
}

/// What an object of the kind is called in a sentence about what still uses a domain.
fn kind_words(kinds: &BTreeSet<String>) -> String {
    let words: Vec<String> = kinds
        .iter()
        .map(|k| match k.as_str() {
            "Account" => "accounts".to_string(),
            "MailingList" => "mailing lists".to_string(),
            "DkimSignature" => "DKIM keys".to_string(),
            other => other.to_lowercase(),
        })
        .collect();
    if words.is_empty() {
        return "other objects on the server".to_string();
    }
    words.join(", ")
}

/// The objects an `objectIsLinked` refusal names: their kinds, and the ids of one kind.
fn linked(e: &Value) -> (BTreeSet<String>, Vec<String>) {
    let mut kinds = BTreeSet::new();
    let mut keys = Vec::new();
    for l in e["linkedObjects"].as_array().into_iter().flatten() {
        let kind = l["object"].as_str().unwrap_or("object").to_string();
        if kind == "DkimSignature" {
            if let Some(id) = l["id"].as_str() {
                keys.push(id.to_string());
            }
        }
        kinds.insert(kind);
    }
    (kinds, keys)
}

impl Connection {
    /// The domains, by name.
    pub(crate) async fn domain_rows(&self) -> Result<Vec<DomainRow>> {
        let args = json!({ "accountId": self.account_id(), "ids": null, "properties": ROW_PROPERTIES });
        let responses = self.admin_request("see the domains", &[("x:Domain/get", args)]).await?;
        let mut rows: Vec<DomainRow> = response(&responses, "x:Domain/get")?["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(domain_row)
            .collect();
        rows.sort_by_cached_key(|d| (d.name.to_lowercase(), d.id.clone()));
        Ok(rows)
    }

    /// One domain with its DNS records and its DKIM keys. A key that may not read the DKIM
    /// objects still gets the domain (`dkim_allowed` says it).
    pub(crate) async fn domain_detail(&self, id: &str) -> Result<DomainDetail> {
        let account = self.account_id();
        let get = json!({ "accountId": account, "ids": [id], "properties": DETAIL_PROPERTIES });
        let query = json!({ "accountId": account, "filter": { "domainId": id }, "limit": 100 });
        let keys = json!({
            "accountId": account, "properties": KEY_PROPERTIES,
            "#ids": { "resultOf": "c0", "name": "x:DkimSignature/query", "path": "/ids" },
        });
        // One request for the domain; the keys are a second one, as a method error does not say which call it was.
        let responses = self.admin_request("see the domains", &[("x:Domain/get", get)]).await?;
        let list = response(&responses, "x:Domain/get")?["list"].as_array().cloned().unwrap_or_default();
        let Some(domain) = list.first() else { return Err(Error::invalid(GONE)) };
        let row = domain_row(domain).ok_or_else(|| Error::invalid(GONE))?;
        let calls = [("x:DkimSignature/query", query), ("x:DkimSignature/get", keys)];
        let answers = self.call_keeping_errors(&[], &calls).await?;
        let (dkim_allowed, mut dkim) = match answers.iter().find(|(n, _)| n == "error") {
            Some((_, e)) if e["type"] == "forbidden" => (false, Vec::new()),
            Some((_, e)) => return Err(Error::temporary(format!("the keys could not be read: {}", text(e, "type")))),
            None => {
                let list = response(&answers, "x:DkimSignature/get")?["list"].as_array().cloned().unwrap_or_default();
                (true, list.iter().filter_map(dkim_key).collect::<Vec<_>>())
            }
        };
        dkim.sort_by(|a, b| a.selector.cmp(&b.selector).then_with(|| a.id.cmp(&b.id)));
        Ok(DomainDetail {
            id: row.id,
            name: row.name,
            enabled: row.enabled,
            aliases: row.aliases,
            dns_zone: text(domain, "dnsZoneFile"),
            dkim_automatic: domain["dkimManagement"]["@type"] == "Automatic",
            dkim_allowed,
            dkim,
        })
    }

    pub(crate) async fn domain_add(&self, name: &str) -> Result<DomainRow> {
        let name = clean_name(name)?;
        let args = json!({ "accountId": self.account_id(), "create": { CREATE_ID: { "name": name } } });
        let responses = self.admin_request("add domains", &[("x:Domain/set", args)]).await?;
        let set = response(&responses, "x:Domain/set")?;
        let id = match set["notCreated"].get(CREATE_ID) {
            Some(e) if e["type"] == "primaryKeyViolation" => {
                return Err(Error::invalid("this domain is already on the server"))
            }
            Some(e) if e["type"] == "invalidPatch" || e["type"] == "invalidProperties" => {
                return Err(Error::invalid("the server does not take this domain name"));
            }
            Some(e) => return Err(set_refusal("add domains", e)),
            None => created_id(set)?,
        };
        let args = json!({ "accountId": self.account_id(), "ids": [id], "properties": ROW_PROPERTIES });
        let responses = self.admin_request("see the domains", &[("x:Domain/get", args)]).await?;
        let list = response(&responses, "x:Domain/get")?["list"].as_array().cloned().unwrap_or_default();
        list.first().and_then(domain_row).ok_or_else(|| Error::temporary("the new domain could not be read back"))
    }

    pub(crate) async fn domain_enable(&self, id: &str, enabled: bool) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "update": { id: { "enable": enabled } } });
        let responses = self.admin_request("change domains", &[("x:Domain/set", args)]).await?;
        let set = response(&responses, "x:Domain/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(if e["type"] == "notFound" { Error::invalid(GONE) } else { set_refusal("change domains", e) });
        }
        updated(set, id, GONE)
    }

    /// Deletes a domain that nothing uses. The guard is here, not in the host: accounts (their
    /// address, and the aliases of any account) are looked at first, and a key that cannot look
    /// does not delete. The domain's own DKIM keys go with it, and only when they are all the
    /// server says is left.
    pub(crate) async fn domain_delete(&self, id: &str) -> Result<()> {
        self.refuse_when_in_use(id).await?;
        let account = self.account_id();
        let destroy = |ids: &[&str]| json!({ "accountId": account, "destroy": ids });
        let responses = self.admin_request("delete domains", &[("x:Domain/set", destroy(&[id]))]).await?;
        let set = response(&responses, "x:Domain/set")?;
        let Some(e) = set["notDestroyed"].get(id) else { return destroyed(set, id) };
        if e["type"] != "objectIsLinked" {
            return match e["type"].as_str() {
                Some("notFound") => Ok(()),
                _ => Err(set_refusal("delete domains", e)),
            };
        }
        let (kinds, keys) = linked(e);
        if kinds.len() != 1 || !kinds.contains("DkimSignature") {
            let others: BTreeSet<String> = kinds.into_iter().filter(|k| k != "DkimSignature").collect();
            return Err(Error::invalid(format!("this domain is still used by {}", kind_words(&others))));
        }
        // Only its keys are left: they go first, in the same request as the domain.
        let key_ids: Vec<&str> = keys.iter().map(String::as_str).collect();
        let calls = [("x:DkimSignature/set", destroy(&key_ids)), ("x:Domain/set", destroy(&[id]))];
        let responses = self.admin_request("delete domains", &calls).await?;
        let keys_set = response(&responses, "x:DkimSignature/set")?;
        if let Some(e) = keys_set["notDestroyed"].as_object().and_then(|m| m.values().find(|e| e["type"] != "notFound"))
        {
            return Err(set_refusal("delete the domain's DKIM keys", e));
        }
        let set = response(&responses, "x:Domain/set")?;
        match set["notDestroyed"].get(id) {
            // The keys are gone by now (JMAP has no transaction across the two calls): say so, the domain signs no more.
            Some(e) if e["type"] == "objectIsLinked" => Err(Error::invalid(format!(
                "its DKIM keys were deleted, but the domain is still used by {}: make new keys if it stays",
                kind_words(&linked(e).0)
            ))),
            Some(e) if e["type"] == "notFound" => Ok(()),
            Some(e) => Err(Error::invalid(format!(
                "its DKIM keys were deleted, but the domain was not: {}",
                set_refusal("delete domains", e)
            ))),
            None => destroyed(set, id),
        }
    }

    /// `Error::Invalid` when an account still has an address or an alias in the domain.
    async fn refuse_when_in_use(&self, id: &str) -> Result<()> {
        let account = self.account_id();
        let query = json!({ "accountId": account, "filter": { "domainId": id }, "limit": 1, "calculateTotal": true });
        let responses = self.admin_request("see which accounts use the domain", &[("x:Account/query", query)]).await?;
        let answer = response(&responses, "x:Account/query")?;
        let used = answer["total"].as_u64().unwrap_or_else(|| answer["ids"].as_array().map_or(0, |i| i.len() as u64));
        if used > 0 {
            return Err(Error::invalid(format!(
                "{used} account{} still use this domain: delete them or give them another address first",
                if used == 1 { "" } else { "s" }
            )));
        }
        let all =
            self.all_ids("x:Account", &json!({ "accountId": account }), "see which accounts use the domain").await?;
        if all.len() >= LIST_MAX {
            return Err(Error::invalid("there are too many accounts to check that none uses this domain"));
        }
        for chunk in all.chunks(self.get_batch()) {
            let args = json!({ "accountId": account, "ids": chunk, "properties": ["aliases"] });
            let responses = self.admin_request("see which accounts use the domain", &[("x:Account/get", args)]).await?;
            for item in response(&responses, "x:Account/get")?["list"].as_array().into_iter().flatten() {
                // Fails closed: aliases in a shape this does not know (or not sent at all) are not "no alias".
                let aliases = &item["aliases"];
                let uses = match aliases {
                    Value::Object(m) => m.values().any(|a| a["domainId"] == id),
                    Value::Array(list) => list.iter().any(|a| a["domainId"] == id),
                    Value::Null if item.get("aliases").is_some() => false,
                    _ => return Err(Error::invalid("the server did not say which aliases the accounts have")),
                };
                if uses {
                    return Err(Error::invalid("an account still has an alias in this domain: remove the alias first"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_domain_name_is_checked_and_made_lower_case() {
        assert_eq!(clean_name("  Example.ORG. ").unwrap(), "example.org");
        assert_eq!(clean_name("xn--bcher-kva.example").unwrap(), "xn--bcher-kva.example");
        for bad in ["", "a b.test", "-a.test", "a-.test", "a..test", ".test", "ü.test", "a@b.test", &"x".repeat(64)] {
            assert!(clean_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn aliases_come_as_a_set_or_a_list() {
        assert_eq!(names(&json!({ "b.test": true, "a.test": true, "c.test": false })), ["a.test", "b.test"]);
        assert_eq!(names(&json!(["z.test", "y.test"])), ["y.test", "z.test"]);
        assert!(names(&Value::Null).is_empty());
    }
}
