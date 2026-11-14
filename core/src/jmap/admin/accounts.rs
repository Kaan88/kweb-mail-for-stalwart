//! The server's accounts and groups over `x:Account` (see `dev/notes/stalwart-admin-access.md`,
//! sections 4 and 6). What matters here:
//!
//! - `@type` is `User` or `Group`; it is only in an answer when asked for as a property.
//! - A user has roles (`Admin` makes an administrator) and credentials; a group has neither.
//! - There is no "enabled" flag. Signing in is switched off by disabling the `authenticate`
//!   permission, so a suspended account has a permission list that lacks it.
//! - The text filter matches whole words of name and description only, so a search reads the
//!   list and filters here. Order is by `name`.
//! - The server takes almost any name, so the name is checked before it is asked.
//! - Passwords: patched at `credentials/<key>/secret`. A password never goes into an error.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::{set_refusal, text, PAGE_MAX};
use crate::{
    ffi::{ServerAccount, ServerAccountKind, ServerAccountPage, ServerDomain},
    jmap::{
        send::{created_id, destroyed, response, set_error, updated, CREATE_ID},
        Connection,
    },
    Error, Result,
};

const GONE: &str = "that account is no longer on the server";
const MIN_PASSWORD: usize = 8;
const NAME_MAX: usize = 64;
const PROPERTIES: [&str; 11] = [
    "name",
    "domainId",
    "emailAddress",
    "description",
    "@type",
    "roles",
    "permissions",
    "quotas",
    "usedDiskQuota",
    "aliases",
    "memberGroupIds",
];

/// Whether the name is one the app lets an administrator make: letters, digits, dot, hyphen
/// and underscore.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_MAX
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn password_ok(password: &str) -> bool {
    password.chars().count() >= MIN_PASSWORD
}

fn short_password() -> Error {
    Error::invalid("the password needs at least 8 characters")
}

/// Why the server turned a password down, in plain words. The server's rule is a setting (a
/// shortest length, a longest length, a minimum strength) and it says which in one of three
/// fixed sentences; they are read here and never passed on as they are, so a sentence that
/// might hold the password (it never does) cannot reach the host.
fn secret_refusal(description: &str, password: &str) -> Error {
    let d = description.to_lowercase();
    let number = |after: &str| {
        let rest = d.split_once(after)?.1;
        let digits: String = rest.trim_start().chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    };
    let repeats_it = !password.is_empty() && description.contains(password);
    Error::invalid(if repeats_it {
        "the server does not take this password".to_string()
    } else if d.contains("at most") {
        match number("at most") {
            Some(n) => format!("the password is too long: at most {n} characters"),
            None => "the password is too long".to_string(),
        }
    } else if d.contains("at least") {
        match number("at least") {
            Some(n) => format!("the password needs at least {n} characters"),
            None => "the password is too short".to_string(),
        }
    } else if d.contains("too weak") {
        "the server finds this password too weak: use a longer one with more kinds of characters".to_string()
    } else {
        "the server does not take this password".to_string()
    })
}

/// What a read needs besides the accounts themselves: the domains' names (an alias is a name
/// and a domain) and the names of the groups the accounts are in.
struct Names {
    domains: HashMap<String, String>,
    groups: HashMap<String, String>,
}

impl Connection {
    /// The domains an account can be made in, by name.
    pub(crate) async fn domain_list(&self) -> Result<Vec<ServerDomain>> {
        let args = json!({ "accountId": self.account_id(), "ids": null, "properties": ["name"] });
        let responses = self.admin_request("see the domains", &[("x:Domain/get", args)]).await?;
        let mut domains: Vec<ServerDomain> = response(&responses, "x:Domain/get")?["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| Some(ServerDomain { id: d["id"].as_str()?.to_string(), name: text(d, "name") }))
            .collect();
        domains.sort_by_cached_key(|d| (d.name.to_lowercase(), d.id.clone()));
        Ok(domains)
    }

    /// Accounts and groups whose name, address or description has every word of `search`
    /// (blank: all), by name.
    pub(crate) async fn account_page(&self, search: &str, offset: usize, limit: usize) -> Result<ServerAccountPage> {
        let account = self.account_id();
        let limit = limit.min(PAGE_MAX);
        let words: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
        let sort = json!([{ "property": "name", "isAscending": true }]);
        if words.is_empty() {
            let query = json!({
                "accountId": account, "sort": sort, "position": offset, "limit": limit, "calculateTotal": true,
            });
            let responses = self.admin_request("see the accounts", &[("x:Account/query", query)]).await?;
            let answer = response(&responses, "x:Account/query")?;
            let ids = string_list(&answer["ids"]);
            let total = answer["total"].as_u64().map_or(ids.len(), |t| t as usize);
            let accounts = self.accounts_by_ids(&ids).await?;
            return Ok(ServerAccountPage { accounts, total: total.min(u32::MAX as usize) as u32 });
        }
        // The server matches whole words only, so a search reads the whole list.
        let ids = self.all_ids("x:Account", &json!({ "accountId": account, "sort": sort }), "see the accounts").await?;
        let mut found = Vec::new();
        for chunk in ids.chunks(self.get_batch()) {
            let args =
                json!({ "accountId": account, "ids": chunk, "properties": ["name", "emailAddress", "description"] });
            let responses = self.admin_request("see the accounts", &[("x:Account/get", args)]).await?;
            for item in response(&responses, "x:Account/get")?["list"].as_array().into_iter().flatten() {
                let haystack =
                    format!("{} {} {}", text(item, "name"), text(item, "emailAddress"), text(item, "description"));
                let haystack = haystack.to_lowercase();
                if let (true, Some(id)) = (words.iter().all(|w| haystack.contains(w)), item["id"].as_str()) {
                    found.push((text(item, "name").to_lowercase(), id.to_string()));
                }
            }
        }
        found.sort();
        let total = found.len().min(u32::MAX as usize) as u32;
        let page: Vec<String> = found.into_iter().skip(offset).take(limit).map(|(_, id)| id).collect();
        Ok(ServerAccountPage { accounts: self.accounts_by_ids(&page).await?, total })
    }

    pub(crate) async fn account_one(&self, id: &str) -> Result<ServerAccount> {
        self.accounts_by_ids(&[id.to_string()]).await?.pop().ok_or_else(|| Error::invalid(GONE))
    }

    /// The accounts with these ids, in this order; an id that is gone is left out.
    async fn accounts_by_ids(&self, ids: &[String]) -> Result<Vec<ServerAccount>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let account = self.account_id();
        let get = json!({ "accountId": account, "ids": ids, "properties": PROPERTIES });
        let responses = self.admin_request("see the accounts", &[("x:Account/get", get)]).await?;
        let list = response(&responses, "x:Account/get")?["list"].as_array().cloned().unwrap_or_default();
        let mut names = Names { domains: self.domain_names(&account).await?, groups: HashMap::new() };
        // The names of the groups, in one more get.
        let mut group_ids: Vec<String> = list
            .iter()
            .flat_map(|a| a["memberGroupIds"].as_object().into_iter().flat_map(|m| m.keys().cloned()))
            .collect();
        group_ids.sort();
        group_ids.dedup();
        if !group_ids.is_empty() {
            let args = json!({ "accountId": account, "ids": group_ids, "properties": ["name"] });
            let responses = self.admin_request("see the accounts", &[("x:Account/get", args)]).await?;
            names.groups = response(&responses, "x:Account/get")?["list"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| Some((g["id"].as_str()?.to_string(), text(g, "name"))))
                .collect();
        }
        let mut accounts: Vec<(usize, ServerAccount)> = list
            .iter()
            .filter_map(|a| Some((ids.iter().position(|id| a["id"].as_str() == Some(id))?, server_account(a, &names)?)))
            .collect();
        accounts.sort_by_key(|(place, _)| *place);
        Ok(accounts.into_iter().map(|(_, a)| a).collect())
    }

    /// The domains' names, for the alias addresses. A key that may not read domains gets none
    /// (the aliases are then left out); that is not a reason to refuse the accounts.
    async fn domain_names(&self, account: &str) -> Result<HashMap<String, String>> {
        let args = json!({ "accountId": account, "ids": null, "properties": ["name"] });
        let responses = self.call_keeping_errors(&[], &[("x:Domain/get", args)]).await?;
        let names = responses
            .iter()
            .filter(|(name, _)| name == "x:Domain/get")
            .filter_map(|(_, answer)| answer["list"].as_array())
            .flatten()
            .filter_map(|d| Some((d["id"].as_str()?.to_string(), text(d, "name"))))
            .collect();
        Ok(names)
    }

    pub(crate) async fn account_create(
        &self,
        name: &str,
        domain_id: &str,
        description: &str,
        password: &str,
    ) -> Result<ServerAccount> {
        let name = name.trim();
        if !name_ok(name) {
            return Err(Error::invalid(
                "the name may have letters, digits, dots, hyphens and underscores only, and at most 64 of them",
            ));
        }
        if !password_ok(password) {
            return Err(short_password());
        }
        let mut new = json!({
            "@type": "User", "name": name, "domainId": domain_id, "roles": { "@type": "User" },
            "credentials": { "0": { "@type": "Password", "secret": password } },
        });
        if !description.trim().is_empty() {
            new["description"] = json!(description.trim());
        }
        let args = json!({ "accountId": self.account_id(), "create": { CREATE_ID: new } });
        let responses = self.admin_request("make accounts", &[("x:Account/set", args)]).await?;
        let set = response(&responses, "x:Account/set")?;
        let id = match set["notCreated"].get(CREATE_ID) {
            Some(e) => return Err(create_refusal(e, password)),
            None => created_id(set)?,
        };
        self.account_one(&id).await
    }

    pub(crate) async fn account_update(
        &self,
        id: &str,
        description: &str,
        quota: Option<u64>,
    ) -> Result<ServerAccount> {
        let description = description.trim();
        let patch = json!({
            "description": if description.is_empty() { Value::Null } else { json!(description) },
            "quotas/maxDiskQuota": quota,
        });
        self.account_patch(id, patch.as_object().cloned().unwrap_or_default(), "").await?;
        self.account_one(id).await
    }

    /// Gives the account's password a new value: the first password credential it has.
    pub(crate) async fn account_password(&self, id: &str, password: &str) -> Result<()> {
        if !password_ok(password) {
            return Err(short_password());
        }
        let account = self.account_id();
        let args = json!({ "accountId": account, "ids": [id], "properties": ["@type", "credentials"] });
        let responses = self.admin_request("see the accounts", &[("x:Account/get", args)]).await?;
        let list = response(&responses, "x:Account/get")?["list"].as_array().cloned().unwrap_or_default();
        let Some(found) = list.first() else { return Err(Error::invalid(GONE)) };
        let credentials = found["credentials"].as_object().cloned().unwrap_or_default();
        let mut keys: Vec<&String> =
            credentials.iter().filter(|(_, c)| c["@type"] == "Password").map(|(k, _)| k).collect();
        keys.sort_by_key(|k| (k.parse::<u64>().unwrap_or(u64::MAX), (*k).clone()));
        let Some(key) = keys.first() else { return Err(Error::invalid("this account has no password to change")) };
        let mut patch = Map::new();
        patch.insert(format!("credentials/{key}/secret"), json!(password));
        self.account_patch(id, patch, password).await
    }

    /// Switches signing in off or on, by the `authenticate` permission.
    pub(crate) async fn account_suspend(&self, id: &str, suspended: bool) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "ids": [id], "properties": ["permissions"] });
        let responses = self.admin_request("see the accounts", &[("x:Account/get", args)]).await?;
        let list = response(&responses, "x:Account/get")?["list"].as_array().cloned().unwrap_or_default();
        let Some(found) = list.first() else { return Err(Error::invalid(GONE)) };
        let permissions = &found["permissions"];
        let is_set = |list: &str| permissions[list].get("authenticate").is_some_and(|v| v == true);
        let mut patch = Map::new();
        match (permissions["@type"].as_str().unwrap_or("Inherit"), suspended) {
            ("Inherit", true) => {
                patch.insert(
                    "permissions".into(),
                    json!({ "@type": "Merge", "disabledPermissions": { "authenticate": true } }),
                );
            }
            ("Inherit", false) => return Ok(()),
            ("Replace", true) => {
                patch.insert("permissions/enabledPermissions/authenticate".into(), Value::Null);
            }
            ("Replace", false) => {
                patch.insert("permissions/enabledPermissions/authenticate".into(), json!(true));
            }
            (_, true) => {
                patch.insert("permissions/disabledPermissions/authenticate".into(), json!(true));
            }
            (_, false) if !is_set("disabledPermissions") => return Ok(()),
            (_, false) => {
                // Without the one disabled permission the list is empty: back to inheriting.
                let others =
                    |list: &str| permissions[list].as_object().is_some_and(|m| m.keys().any(|k| k != "authenticate"));
                if others("disabledPermissions") || others("enabledPermissions") {
                    patch.insert("permissions/disabledPermissions/authenticate".into(), Value::Null);
                } else {
                    patch.insert("permissions".into(), json!({ "@type": "Inherit" }));
                }
            }
        }
        self.account_patch(id, patch, "").await
    }

    pub(crate) async fn account_delete(&self, id: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "destroy": [id] });
        let responses = self.admin_request("delete accounts", &[("x:Account/set", args)]).await?;
        let set = response(&responses, "x:Account/set")?;
        if let Some(e) = set["notDestroyed"].get(id) {
            return Err(match e["type"].as_str() {
                Some("objectIsLinked") => linked_refusal(e),
                Some("forbidden") => set_refusal("delete accounts", e),
                Some("notFound") => Error::invalid(GONE),
                _ => set_error(e),
            });
        }
        destroyed(set, id)
    }

    /// One account's patch, and why it did not apply.
    async fn account_patch(&self, id: &str, patch: Map<String, Value>, password: &str) -> Result<()> {
        let args = json!({ "accountId": self.account_id(), "update": { id: patch } });
        let responses = self.admin_request("change accounts", &[("x:Account/set", args)]).await?;
        let set = response(&responses, "x:Account/set")?;
        if let Some(e) = set["notUpdated"].get(id) {
            return Err(match e["type"].as_str() {
                Some("notFound") => Error::invalid(GONE),
                Some("forbidden") => set_refusal("change accounts", e),
                Some("invalidProperties")
                    if e["properties"].as_array().is_some_and(|p| p.iter().any(|p| p == "secret")) =>
                {
                    secret_refusal(e["description"].as_str().unwrap_or_default(), password)
                }
                _ => set_error(e),
            });
        }
        updated(set, id, GONE)
    }
}

fn string_list(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

/// Why an account was not made, in plain words.
fn create_refusal(e: &Value, password: &str) -> Error {
    match e["type"].as_str() {
        Some("primaryKeyViolation") => Error::invalid("that name is already taken"),
        Some("invalidForeignKey") => Error::invalid("the server has no such domain"),
        Some("forbidden") => set_refusal("make accounts", e),
        Some("overQuota") => Error::invalid("the server's limit of accounts is reached"),
        Some("invalidProperties") | Some("invalidPatch") => {
            let props = e["properties"].as_array();
            let on = |name: &str| props.is_some_and(|p| p.iter().any(|p| p == name));
            if on("secret") {
                return secret_refusal(e["description"].as_str().unwrap_or_default(), password);
            }
            Error::invalid(if on("name") {
                "the server does not take that name"
            } else {
                "the server does not take those details"
            })
        }
        _ => set_error(e),
    }
}

/// Why an account is still needed: a group's members, or something else.
fn linked_refusal(e: &Value) -> Error {
    let by_accounts =
        e["linkedObjects"].as_array().is_some_and(|l| !l.is_empty() && l.iter().all(|o| o["object"] == "Account"));
    Error::invalid(if by_accounts {
        "this group still has members: take them out of it first"
    } else {
        "something on the server still uses this account"
    })
}

fn server_account(a: &Value, names: &Names) -> Option<ServerAccount> {
    let id = a["id"].as_str()?.to_string();
    let kind = if a["@type"] == "Group" { ServerAccountKind::Group } else { ServerAccountKind::User };
    let permissions = &a["permissions"];
    let disabled = |list: &str| permissions[list].get("authenticate").is_some_and(|v| v == true);
    let is_suspended = match permissions["@type"].as_str() {
        Some("Merge") => disabled("disabledPermissions"),
        Some("Replace") => permissions["enabledPermissions"].get("authenticate").is_none(),
        _ => false,
    };
    let mut aliases: Vec<String> = a["aliases"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter(|alias| alias["enable"] != false)
        .filter_map(|alias| {
            let domain = names.domains.get(alias["domainId"].as_str()?)?;
            Some(format!("{}@{domain}", alias["name"].as_str()?))
        })
        .collect();
    aliases.sort();
    let mut groups: Vec<String> = a["memberGroupIds"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .filter_map(|id| names.groups.get(id).cloned())
        .collect();
    groups.sort_by_key(|g| g.to_lowercase());
    Some(ServerAccount {
        id,
        name: text(a, "name"),
        email: text(a, "emailAddress"),
        description: text(a, "description"),
        kind,
        is_admin: kind == ServerAccountKind::User && a["roles"]["@type"] == "Admin",
        is_suspended,
        quota: a["quotas"]["maxDiskQuota"].as_u64(),
        used: a["usedDiskQuota"].as_u64(),
        aliases,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked_here() {
        assert!(name_ok("ann.lee-2_x"));
        for bad in ["", "a b", "a@b", "Bad Name", "ünï", &"x".repeat(65)] {
            assert!(!name_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_suspended_account_is_one_whose_authenticate_permission_is_off() {
        let names = Names { domains: HashMap::new(), groups: HashMap::new() };
        let account = |permissions: Value| {
            server_account(&json!({ "id": "x", "@type": "User", "permissions": permissions }), &names).unwrap()
        };
        assert!(!account(json!({ "@type": "Inherit" })).is_suspended);
        assert!(account(json!({ "@type": "Merge", "disabledPermissions": { "authenticate": true } })).is_suspended);
        assert!(!account(json!({ "@type": "Merge", "disabledPermissions": {} })).is_suspended);
        assert!(account(json!({ "@type": "Replace", "enabledPermissions": { "emailReceive": true } })).is_suspended);
    }
}
