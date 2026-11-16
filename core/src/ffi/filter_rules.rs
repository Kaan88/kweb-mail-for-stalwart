//! Filters that share Bulwark's script (M12 stage 2a). The rules travel as Bulwark's JSON: a list
//! of `{id, name, enabled, matchType, conditions, actions, stopProcessing, includeSpam?,
//! activeFrom?, activeUntil?}` (see `dev/notes/bulwark-filters.md`). The core keeps the keys it
//! does not know and the external section of the script; the host sends whole rules back.
//!
//! Every write needs a [`Core::filters_state`] read in this run and fails when the scripts
//! changed on the server since ("the filters changed elsewhere"): read again then.

use std::sync::Arc;

use super::Core;
use crate::{
    filters::bulwark::{
        store::{self, Edit, Kind, Loaded},
        Rule,
    },
    jmap::Connection,
    Error, Result,
};

/// What the server allows.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FilterCapabilities {
    pub regex: bool,
    pub body: bool,
    pub mime: bool,
    /// Active periods (the `date` and `relational` extensions).
    pub period: bool,
    pub mailboxid: bool,
    /// Move and copy rules can leave spam alone.
    pub spamguard: bool,
    /// The script can include the away message's script.
    pub include: bool,
    /// Forwards one message may cause: `Some(0)` hides forwarding; `None` when not said.
    pub redirect_limit: Option<u32>,
    /// The most forwards one message can collect through the rules as they are.
    pub max_forwards_now: u32,
    /// The largest script in bytes, when the server says.
    pub max_size: Option<u64>,
}

/// Rules another tool made, kept as text and shown read-only.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ExternalBlock {
    /// Roundcube, Nextcloud, Horde, Kolab, Dovecot, Thunderbird or External.
    pub origin_label: String,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FiltersState {
    /// False when the server has no filter scripts; everything else is then empty.
    pub supported: bool,
    /// The script the rules live in; empty when there is none yet.
    pub script_name: String,
    /// That script is the one the server runs.
    pub active: bool,
    /// A metadata comment that cannot be read as rules: `raw_text` is shown, nothing is rewritten.
    pub opaque: bool,
    /// A script that has no metadata comment (another tool's): `raw_text` is shown, never rewritten.
    pub foreign: bool,
    /// The script's text when it is opaque or foreign.
    pub raw_text: String,
    /// Bulwark's JSON list of rules (the old script's, converted, while `migration_pending`).
    pub rules_json: String,
    /// The external section (empty when none).
    pub external_blocks: Vec<ExternalBlock>,
    pub capabilities: FilterCapabilities,
    /// The script runs the away message too.
    pub vacation_included: bool,
    /// The away message is on and cannot run beside the filters: switching the filters on
    /// switches it off.
    pub vacation_conflict: bool,
    /// The away message was switched on elsewhere and switched the filters off.
    pub displaced: bool,
    /// The rule part of the script was written by another version of the editor: saving writes it
    /// the way this one does.
    pub differs: bool,
    /// A `kweb-filters` script from before exists.
    pub old_script_present: bool,
    /// Its rules in the new model (JSON list), when it can be read.
    pub old_rules_json: String,
    /// There is no new script yet: `rules_json` are the old rules, to be saved with
    /// [`Core::filters_save`]; then [`Core::filters_migrate_old`] removes the old script.
    pub migration_pending: bool,
    /// The old script is the one the server runs.
    pub old_active: bool,
    /// The away card's forwarding is on (it counts against the forward limit).
    pub vacation_forward: bool,
}

/// Which way a rule moves in the list.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum RuleMove {
    Up,
    Down,
}

fn origin_label(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("rule:[") || lower.contains("roundcube") || lower.contains("managesieve") {
        "Roundcube"
    } else if lower.contains("nextcloud") {
        "Nextcloud"
    } else if lower.contains("horde") || lower.contains("ingo") {
        "Horde"
    } else if lower.contains("kolab") {
        "Kolab"
    } else if lower.contains("dovecot") {
        "Dovecot"
    } else if lower.contains("thunderbird") {
        "Thunderbird"
    } else {
        "External"
    }
}

fn to_state(loaded: Loaded) -> FiltersState {
    let v = loaded.view;
    let c = &v.caps;
    let external_blocks = match v.external.is_empty() {
        true => Vec::new(),
        false => vec![ExternalBlock { origin_label: origin_label(&v.external).to_string(), raw: v.external.clone() }],
    };
    FiltersState {
        supported: true,
        script_name: v.script_name,
        active: v.active,
        opaque: v.kind == Kind::Opaque,
        foreign: v.kind == Kind::Foreign,
        raw_text: v.raw_text,
        rules_json: store::rules_to_json(&v.rules),
        external_blocks,
        capabilities: FilterCapabilities {
            regex: c.regex,
            body: c.body,
            mime: c.mime,
            period: c.period,
            mailboxid: c.mailboxid,
            spamguard: c.spamguard,
            include: c.include,
            redirect_limit: c.redirect_limit.map(|n| n.min(u64::from(u32::MAX)) as u32),
            max_forwards_now: c.max_forwards_now,
            max_size: c.max_size,
        },
        vacation_included: v.vacation_included,
        vacation_conflict: v.vacation_conflict,
        displaced: v.displaced,
        differs: v.differs,
        old_script_present: v.old_script_present,
        old_rules_json: store::rules_to_json(&v.old_rules),
        migration_pending: v.migration_pending,
        old_active: v.old_active,
        vacation_forward: v.vacation_forward,
    }
}

fn unsupported() -> FiltersState {
    FiltersState {
        supported: false,
        script_name: String::new(),
        active: false,
        opaque: false,
        foreign: false,
        raw_text: String::new(),
        rules_json: "[]".into(),
        external_blocks: Vec::new(),
        capabilities: FilterCapabilities {
            regex: false,
            body: false,
            mime: false,
            period: false,
            mailboxid: false,
            spamguard: false,
            include: false,
            redirect_limit: None,
            max_forwards_now: 0,
            max_size: None,
        },
        vacation_included: false,
        vacation_conflict: false,
        displaced: false,
        differs: false,
        old_script_present: false,
        old_rules_json: "[]".into(),
        migration_pending: false,
        old_active: false,
        vacation_forward: false,
    }
}

impl Core {
    fn filter_rules_result(&self, result: Result<Loaded>) -> Result<FiltersState> {
        match result {
            Ok(loaded) => {
                self.lock().filters_state = Some(loaded.state.clone());
                Ok(to_state(loaded))
            }
            Err(Error::Temporary { msg }) if msg == crate::filters::SAVED_NOT_READ => {
                self.lock().filters_state = None;
                Err(Error::Temporary { msg })
            }
            Err(e) => Err(e),
        }
    }

    fn filter_rules_write<F, Fut>(&self, f: F) -> Result<FiltersState>
    where
        F: Fn(Arc<Connection>, String) -> Fut,
        Fut: std::future::Future<Output = Result<Loaded>>,
    {
        let base = self.filters_base()?;
        let result = self.with_connection(|conn| f(conn, base.clone()));
        self.filter_rules_result(result)
    }
}

#[uniffi::export]
impl Core {
    /// The filters as the server has them now, in Bulwark's model. A server without filter scripts
    /// gives `supported: false`. Asks the server (scripts, the script's text, the folders when an
    /// old script is to be converted).
    pub fn filters_state(&self) -> Result<FiltersState> {
        self.account_id()?;
        match self.with_connection(|conn| async move { store::load(&conn).await }) {
            Ok(loaded) => self.filter_rules_result(Ok(loaded)),
            Err(Error::Invalid { msg }) if msg == "this server has no mail filters" => Ok(unsupported()),
            Err(e) => Err(e),
        }
    }

    /// Writes `rules_json` (the whole list, Bulwark's JSON) as the filters: checks every rule
    /// (`Error::Invalid` naming it), has the server check the script, then writes it. A script that
    /// is opaque or foreign is refused; use [`Core::filters_raw_save`] or
    /// [`Core::filters_reset_to_visual`]. A first save makes the script the one the server runs;
    /// a later one keeps it as it is. Returns the state read back. A rule the host sends with an id
    /// the script has keeps the keys of its own the host did not send.
    pub fn filters_save(&self, rules_json: String) -> Result<FiltersState> {
        let rules: Vec<Rule> = store::rules_from_json(&rules_json)?;
        self.filter_rules_write(|conn, base| {
            let rules = rules.clone();
            async move { store::save(&conn, Some(&base), rules).await }
        })
    }

    /// The "Use these filters" switch: on makes the script the one the server runs (and, when the
    /// away message is on, includes it); off stops the filters. Nothing is rewritten unless the
    /// away message has to be included. No script yet is `Error::Invalid`.
    pub fn filters_set_enabled(&self, enabled: bool) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| async move { store::set_enabled(&conn, Some(&base), enabled).await })
    }

    /// Switches one rule on or off, and saves.
    pub fn filters_rule_toggle(&self, id: String, enabled: bool) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| {
            let id = id.clone();
            async move { store::edit(&conn, Some(&base), &id, Edit::Toggle(enabled)).await }
        })
    }

    /// Moves one rule one place, and saves.
    pub fn filters_rule_move(&self, id: String, direction: RuleMove) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| {
            let id = id.clone();
            let how = match direction {
                RuleMove::Up => Edit::Up,
                RuleMove::Down => Edit::Down,
            };
            async move { store::edit(&conn, Some(&base), &id, how).await }
        })
    }

    /// Deletes one rule, and saves. The host offers Undo by saving the rule list it had.
    pub fn filters_rule_delete(&self, id: String) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| {
            let id = id.clone();
            async move { store::edit(&conn, Some(&base), &id, Edit::Delete).await }
        })
    }

    /// The raw editor: has the server check `text`, then writes it as the script as it is. If it
    /// is a script the editor reads (a metadata comment it understands) the rules show again,
    /// otherwise it is opaque or foreign.
    pub fn filters_raw_save(&self, text: String) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| {
            let text = text.clone();
            async move { store::raw_save(&conn, Some(&base), &text).await }
        })
    }

    /// "Reset to the visual builder": the script the editor cannot read as rules is replaced by one
    /// with no rules. The host asks first: this throws the text away.
    pub fn filters_reset_to_visual(&self) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| async move { store::reset_to_visual(&conn, Some(&base)).await })
    }

    /// Removes the old `kweb-filters` script after its rules were saved into the new one. Refused
    /// until there is a readable new script. The host asks first.
    pub fn filters_migrate_old(&self) -> Result<FiltersState> {
        self.filter_rules_write(|conn, base| async move { store::migrate_old(&conn, Some(&base)).await })
    }
}
