//! The mock's second part of the management side: `x:Domain` (with the DNS records and the
//! DKIM keys the server makes by itself), `x:DkimSignature`, the three kinds of received report,
//! `x:Task` and `x:ClusterNode`. Like Stalwart 1.1.0 where that matters to the core (see
//! `dev/notes/stalwart-admin-more.md`):
//!
//! - A domain name is made lower case; a bad one is `invalidPatch`; one that exists is
//!   `primaryKeyViolation`. A new domain gets two DKIM keys (`dkimManagement` is `Automatic`).
//! - A domain that an account, an alias, a key or anything else points at is `objectIsLinked`
//!   with the linked objects named; the keys count as linked objects.
//! - A report query has no sort (`receivedAt` is `unsupportedSort`). A task query sorts only by
//!   `due` and filters by `status`, `@type` and `due`; a task is run again by replacing its
//!   `status`; one that is being run is `forbidden` ("currently being processed").
//! - A property a variant does not have is left out of a get.

use serde_json::{json, Value};

use super::{ids_of, patch, pick, str_of, window, Admin, MethodResult, MockServer, DOMAIN_ID, OTHER_DOMAIN_ID};

/// The two domains the mock starts with, by id.
pub(super) fn start_domains() -> Vec<Value> {
    [(DOMAIN_ID, "example.test"), (OTHER_DOMAIN_ID, "other.test")]
        .iter()
        .map(|(id, name)| domain(id, name, "Manual"))
        .collect()
}

fn domain(id: &str, name: &str, dkim: &str) -> Value {
    json!({
        "id": id, "name": name, "enable": true, "aliases": {},
        "dkimManagement": { "@type": dkim }, "dnsManagement": { "@type": "Manual" },
        "dnsZoneFile": format!(
            "{name}. IN MX 10 mail.{name}.\n{name}. IN TXT \"v=spf1 mx -all\"\n_dmarc.{name}. IN TXT \"v=DMARC1; p=reject\"\n"
        ),
    })
}

pub(super) fn start_cluster() -> Vec<Value> {
    vec![
        json!({ "id": "n1", "nodeId": 1, "hostname": "mail.example.test", "status": "active", "lastRenewal": "2027-01-15T08:00:00Z" }),
    ]
}

fn unsupported(kind: &'static str, what: &str) -> (&'static str, String) {
    (kind, what.to_string())
}

/// `{ "type": ..., "id": ... }` rows to give back in a query, with the filter applied by `keep`.
fn run_query(items: &[Value], args: &Value, keep: impl Fn(&Value) -> bool) -> Value {
    window(items.iter().filter(|i| keep(i)).map(|i| str_of(i, "id").to_string()).collect(), args)
}

impl Admin {
    /// The methods of this part; `None` for any other.
    pub(super) fn more(&mut self, name: &str, args: &Value) -> Option<MethodResult> {
        Some(match name {
            "x:Domain/get" => Ok(Self::get_of(&self.domains, args)),
            "x:Domain/query" => Ok(window(self.domains.iter().map(|d| str_of(d, "id").to_string()).collect(), args)),
            "x:Domain/set" => Ok(self.domain_set(args)),
            "x:DkimSignature/query" => self.dkim_query(args),
            "x:DkimSignature/get" => Ok(Self::get_of(&self.dkim, args)),
            "x:DkimSignature/set" => Ok(Self::destroy_of(&mut self.dkim, args)),
            "x:DmarcExternalReport/query" => Self::report_query(&self.dmarc, args),
            "x:DmarcExternalReport/get" => Ok(Self::get_of(&self.dmarc, args)),
            "x:DmarcExternalReport/set" => Ok(Self::destroy_of(&mut self.dmarc, args)),
            "x:TlsExternalReport/query" => Self::report_query(&self.tls, args),
            "x:TlsExternalReport/get" => Ok(Self::get_of(&self.tls, args)),
            "x:TlsExternalReport/set" => Ok(Self::destroy_of(&mut self.tls, args)),
            "x:ArfExternalReport/query" => Self::report_query(&self.arf, args),
            "x:ArfExternalReport/get" => Ok(Self::get_of(&self.arf, args)),
            "x:ArfExternalReport/set" => Ok(Self::destroy_of(&mut self.arf, args)),
            "x:Task/query" => self.task_query(args),
            "x:Task/get" => Ok(Self::get_of(&self.tasks, args)),
            "x:Task/set" => Ok(self.task_set(args)),
            "x:ClusterNode/query" => {
                Ok(window(self.cluster.iter().map(|n| str_of(n, "id").to_string()).collect(), args))
            }
            "x:ClusterNode/get" => Ok(Self::get_of(&self.cluster, args)),
            _ => return None,
        })
    }

    fn get_of(items: &[Value], args: &Value) -> Value {
        let wanted = ids_of(args, items.iter().map(|i| str_of(i, "id").to_string()));
        let list: Vec<Value> = wanted
            .iter()
            .filter_map(|id| items.iter().find(|i| str_of(i, "id") == id))
            .map(|i| pick(i, args))
            .collect();
        let missing: Vec<&String> =
            wanted.iter().filter(|id| !items.iter().any(|i| str_of(i, "id") == id.as_str())).collect();
        json!({ "list": list, "notFound": missing })
    }

    fn destroy_of(items: &mut Vec<Value>, args: &Value) -> Value {
        let mut out = json!({});
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            match items.iter().position(|i| str_of(i, "id") == id) {
                Some(at) => {
                    items.remove(at);
                    let mut done = out["destroyed"].as_array().cloned().unwrap_or_default();
                    done.push(json!(id));
                    out["destroyed"] = json!(done);
                }
                None => out["notDestroyed"][id] = json!({ "type": "notFound" }),
            }
        }
        out
    }

    // ---- domains and keys ----

    pub(super) fn domain_name(&self, id: &str) -> Option<String> {
        self.domains.iter().find(|d| str_of(d, "id") == id).map(|d| str_of(d, "name").to_string())
    }

    fn domain_set(&mut self, args: &Value) -> Value {
        let mut out = json!({});
        for (client_id, new) in args["create"].as_object().into_iter().flatten() {
            let name = str_of(new, "name").to_lowercase();
            let valid = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
                && !name.starts_with('.');
            if !valid {
                out["notCreated"][client_id] =
                    json!({ "type": "invalidPatch", "description": "Invalid domain name", "properties": ["name"] });
            } else if let Some(taken) = self.domains.iter().find(|d| str_of(d, "name") == name) {
                out["notCreated"][client_id] = json!({
                    "type": "primaryKeyViolation", "properties": ["name"],
                    "objectId": { "object": "Domain", "id": taken["id"] },
                });
            } else {
                let id = self.id("d");
                self.domains.push(domain(&id, &name, "Automatic"));
                for algorithm in ["Dkim1Ed25519Sha256", "Dkim1RsaSha256"] {
                    let key = self.id("k");
                    self.dkim.push(json!({
                        "id": key, "domainId": id, "@type": algorithm, "selector": format!("v1-{name}"),
                        "stage": "active", "createdAt": "2027-01-15T08:00:00Z", "nextTransitionAt": null,
                        "privateKey": { "@type": "Text", "secret": "****" }, "publicKey": "AAAA",
                    }));
                }
                out["created"][client_id] = json!({ "id": id });
            }
        }
        for (id, change) in args["update"].as_object().into_iter().flatten() {
            match self.domains.iter_mut().find(|d| str_of(d, "id") == id) {
                Some(d) => {
                    for (k, v) in change.as_object().into_iter().flatten() {
                        patch(d, k, v);
                    }
                    out["updated"][id] = Value::Null;
                }
                None => out["notUpdated"][id] = json!({ "type": "notFound" }),
            }
        }
        for id in args["destroy"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !self.domains.iter().any(|d| str_of(d, "id") == id) {
                out["notDestroyed"][id] = json!({ "type": "notFound" });
                continue;
            }
            let mut linked: Vec<Value> = Vec::new();
            for a in &self.accounts {
                let alias = a["aliases"].as_object().is_some_and(|m| m.values().any(|x| x["domainId"] == id));
                if a["domainId"] == id || alias {
                    linked.push(json!({ "object": "Account", "id": a["id"] }));
                }
            }
            for k in self.dkim.iter().filter(|k| k["domainId"] == id) {
                linked.push(json!({ "object": "DkimSignature", "id": k["id"] }));
            }
            for kind in self.domain_links.iter().filter(|(d, _)| d == id).map(|(_, k)| k) {
                linked.push(json!({ "object": kind, "id": "x1" }));
            }
            if !linked.is_empty() {
                out["notDestroyed"][id] = json!({
                    "type": "objectIsLinked", "objectId": { "object": "Domain", "id": id }, "linkedObjects": linked,
                });
                continue;
            }
            self.domains.retain(|d| str_of(d, "id") != id);
            let mut done = out["destroyed"].as_array().cloned().unwrap_or_default();
            done.push(json!(id));
            out["destroyed"] = json!(done);
        }
        out
    }

    fn dkim_query(&self, args: &Value) -> MethodResult {
        if let Some(bad) = args["filter"].as_object().and_then(|f| f.keys().find(|k| *k != "domainId")) {
            return Err(unsupported("unsupportedFilter", bad));
        }
        let domain = args["filter"]["domainId"].as_str();
        Ok(run_query(&self.dkim, args, |k| domain.is_none_or(|d| str_of(k, "domainId") == d)))
    }

    // ---- reports ----

    fn report_query(items: &[Value], args: &Value) -> MethodResult {
        if let Some(sort) = args["sort"][0]["property"].as_str() {
            return Err(unsupported("unsupportedSort", &format!("Property {sort} is not supported for sorting")));
        }
        Ok(run_query(items, args, |_| true))
    }

    // ---- tasks ----

    fn task_query(&self, args: &Value) -> MethodResult {
        if let Some(sort) = args["sort"][0]["property"].as_str().filter(|p| *p != "due") {
            return Err(unsupported("unsupportedSort", &format!("Property {sort} is not supported for sorting")));
        }
        if let Some(bad) = args["filter"]
            .as_object()
            .and_then(|f| f.keys().find(|k| !["status", "@type", "due"].contains(&k.as_str())))
        {
            return Err(unsupported(
                "unsupportedFilter",
                &format!("Filter on property {bad} is not supported or invalid"),
            ));
        }
        let status = args["filter"]["status"].as_str();
        let kind = args["filter"]["@type"].as_str();
        let mut items: Vec<&Value> = self.tasks.iter().collect();
        items.sort_by_key(|t| t["due"].as_str().map(str::to_string).unwrap_or_default());
        let ids = items
            .into_iter()
            .filter(|t| status.is_none_or(|s| t["status"]["@type"] == s) && kind.is_none_or(|k| t["@type"] == k))
            .map(|t| str_of(t, "id").to_string())
            .collect();
        Ok(window(ids, args))
    }

    fn task_set(&mut self, args: &Value) -> Value {
        let mut out = json!({});
        for (id, change) in args["update"].as_object().into_iter().flatten() {
            if self.busy_tasks.iter().any(|b| b == id) {
                out["notUpdated"][id] = json!({ "type": "forbidden", "description": "Task is currently being processed and cannot be updated" });
                continue;
            }
            match self.tasks.iter_mut().find(|t| str_of(t, "id") == id) {
                Some(t) => {
                    for (k, v) in change.as_object().into_iter().flatten() {
                        patch(t, k, v);
                    }
                    t["due"] = t["status"]["due"].clone();
                    out["updated"][id] = Value::Null;
                }
                None => out["notUpdated"][id] = json!({ "type": "notFound" }),
            }
        }
        let destroyed = Self::destroy_of(&mut self.tasks, args);
        for key in ["destroyed", "notDestroyed"] {
            if !destroyed[key].is_null() {
                out[key] = destroyed[key].clone();
            }
        }
        out
    }
}

/// Controls for the tests of this part.
impl MockServer {
    /// The domain objects the server holds, by id.
    pub fn domain_ids(&self) -> Vec<String> {
        self.with_admin(|a| a.domains.iter().map(|d| str_of(d, "id").to_string()).collect())
    }

    pub fn server_domain(&self, id: &str) -> Option<Value> {
        self.with_admin(|a| a.domains.iter().find(|d| str_of(d, "id") == id).cloned())
    }

    /// How many DKIM keys the server holds for a domain.
    pub fn dkim_count(&self, domain_id: &str) -> usize {
        self.with_admin(|a| a.dkim.iter().filter(|k| k["domainId"] == domain_id).count())
    }

    /// Makes something else (a mailing list, say) point at a domain, as the server counts it.
    pub fn domain_link(&self, domain_id: &str, kind: &str) {
        self.with_admin(|a| a.domain_links.push((domain_id.to_string(), kind.to_string())));
    }

    /// Gives an account an alias in a domain.
    pub fn account_alias(&self, account_id: &str, name: &str, domain_id: &str) {
        self.with_admin(|a| {
            if let Some(x) = a.accounts.iter_mut().find(|x| str_of(x, "id") == account_id) {
                x["aliases"]["9"] = json!({ "name": name, "domainId": domain_id, "enable": true });
            }
        });
    }

    /// Adds a received report of a kind (`"dmarc"`, `"tls"` or `"arf"`) and gives its id.
    /// `report` has what the server has: `from`, `subject`, `receivedAt`, `report`, ...
    pub fn report_add(&self, kind: &str, mut report: Value) -> String {
        self.with_admin(|a| {
            let id = a.id("r");
            report["id"] = json!(id);
            match kind {
                "dmarc" => a.dmarc.push(report),
                "tls" => a.tls.push(report),
                _ => a.arf.push(report),
            }
            id
        })
    }

    pub fn report_ids(&self, kind: &str) -> Vec<String> {
        self.with_admin(|a| {
            let items = match kind {
                "dmarc" => &a.dmarc,
                "tls" => &a.tls,
                _ => &a.arf,
            };
            items.iter().map(|r| str_of(r, "id").to_string()).collect()
        })
    }

    /// Adds a task and gives its id. `task` has `@type`, `status` (with `due`) and what the type has.
    pub fn task_add(&self, mut task: Value) -> String {
        self.with_admin(|a| {
            let id = a.id("t");
            task["id"] = json!(id);
            task["due"] = task["status"]["due"].clone();
            a.tasks.push(task);
            id
        })
    }

    pub fn server_task(&self, id: &str) -> Option<Value> {
        self.with_admin(|a| a.tasks.iter().find(|t| str_of(t, "id") == id).cloned())
    }

    pub fn task_ids(&self) -> Vec<String> {
        self.with_admin(|a| a.tasks.iter().map(|t| str_of(t, "id").to_string()).collect())
    }

    /// The server is running this task now: it cannot be changed.
    pub fn task_set_busy(&self, id: &str) {
        self.with_admin(|a| a.busy_tasks.push(id.to_string()));
    }

    /// Replaces the cluster's nodes.
    pub fn cluster_set(&self, nodes: Vec<Value>) {
        self.with_admin(|a| a.cluster = nodes);
    }
}
