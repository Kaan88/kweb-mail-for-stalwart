//! The filters on the server, the way Bulwark's store handles them: the script that runs (else
//! the first one that is not the away message's) holds the rules in its metadata comment; the
//! app writes rules, never parses Sieve, keeps the external section as it was, and never
//! writes over a script it cannot read as rules.
//!
//! Every write checks that the scripts are still in the state the host read them at.

use std::collections::HashSet;

use super::{
    check::check_rules,
    forward::max_forwards,
    generate::{bulwark_requires, generate, rule_requires, Options, VACATION_SCRIPT_NAME},
    json::J,
    migrate::convert_all,
    model::{Metadata, Rule},
    read::{read, Parsed, Reading},
};
use crate::{
    filters::{check_state, find, SAVED_NOT_READ},
    jmap::{Connection, SieveList, SieveScript},
    Error, Result,
};

/// The script the app made before it shared Bulwark's (M8).
pub const OLD_SCRIPT: &str = "kweb-filters";
/// What a new script is called, as in Bulwark.
pub const NEW_SCRIPT: &str = "filters";
const NO_FILTERS: &str = "this server has no mail filters";
const LOCKED: &str = "The filter script on the server was written by hand or by another program, and the rules in it \
                      cannot be read. Edit it as text, or start the rules over from the editor.";

/// What the server allows, for the editor to offer only what works.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caps {
    pub regex: bool,
    pub body: bool,
    pub mime: bool,
    pub period: bool,
    pub mailboxid: bool,
    pub spamguard: bool,
    pub include: bool,
    /// `maxNumberRedirects`; `None` when the server does not say.
    pub redirect_limit: Option<u64>,
    /// The most forwards one message can collect through the rules now.
    pub max_forwards_now: u32,
    pub max_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// No script of ours yet.
    None,
    Empty,
    Rules,
    Opaque,
    Foreign,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub script_name: String,
    /// The script is the one the server runs.
    pub active: bool,
    pub kind: Kind,
    /// The script's text when it is opaque or foreign.
    pub raw_text: String,
    pub rules: Vec<Rule>,
    /// The external section, as text ("" when none).
    pub external: String,
    /// The rule part is not what this app would write (see `Parsed::differs`).
    pub differs: bool,
    pub caps: Caps,
    /// The script runs the away message's script too.
    pub vacation_included: bool,
    /// The away message is on and cannot run beside the filters (no `include` here, or a script
    /// the app cannot change): switching the filters on switches it off.
    pub vacation_conflict: bool,
    /// The away message was switched on elsewhere and displaced the filters.
    pub displaced: bool,
    pub old_script_present: bool,
    /// The old script's rules in the new model.
    pub old_rules: Vec<Rule>,
    /// There is no script of the new kind yet and `rules` are the old script's.
    pub migration_pending: bool,
    /// The old script is the one the server runs.
    pub old_active: bool,
    pub vacation_forward: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub view: View,
    pub state: String,
}

struct Inspect {
    list: SieveList,
    max_size: Option<u64>,
    redirects: Option<u64>,
    extensions: Vec<String>,
    chosen: Option<SieveScript>,
    text: String,
    reading: Reading,
    vacation_active: bool,
    old: Option<(SieveScript, String)>,
}

impl Inspect {
    fn has(&self, ext: &str) -> bool {
        self.extensions.iter().any(|e| e == ext)
    }

    fn parsed(&self) -> Option<&Parsed> {
        match &self.reading {
            Reading::Rules(p) => Some(p),
            _ => None,
        }
    }

    fn locked(&self) -> bool {
        matches!(self.reading, Reading::Opaque | Reading::Foreign)
    }

    /// Whether the generated script includes the away message's script.
    fn include_now(&self) -> bool {
        self.has("include") && (self.parsed().is_some_and(|p| p.meta.include_vacation) || self.vacation_active)
    }
}

/// The script the filters live in: the one that runs, else the first; never the away
/// message's, and not the old one.
fn choose(scripts: &[SieveScript]) -> Option<&SieveScript> {
    let ours = |s: &&SieveScript| {
        !s.name.eq_ignore_ascii_case(VACATION_SCRIPT_NAME) && !s.name.eq_ignore_ascii_case(OLD_SCRIPT)
    };
    scripts.iter().filter(ours).find(|s| s.is_active).or_else(|| scripts.iter().find(|s| ours(s)))
}

async fn inspect(conn: &Connection) -> Result<Inspect> {
    if !conn.has_sieve() {
        return Err(Error::invalid(NO_FILTERS));
    }
    let list = conn.sieve_scripts().await?;
    let limits = conn.sieve_limits().await?;
    let chosen = choose(&list.scripts).cloned();
    let (text, reading) = match &chosen {
        Some(script) => {
            let text = conn.sieve_text(script).await?;
            let reading = read(&text, &limits.extensions);
            (text, reading)
        }
        None => (String::new(), Reading::Empty),
    };
    let vacation_active = find(&list.scripts, VACATION_SCRIPT_NAME).is_some_and(|s| s.is_active);
    let old = match find(&list.scripts, OLD_SCRIPT) {
        Some(script) => Some((script.clone(), conn.sieve_text(script).await?)),
        None => None,
    };
    Ok(Inspect {
        list,
        max_size: limits.max_size,
        redirects: limits.max_redirects,
        extensions: limits.extensions,
        chosen,
        text,
        reading,
        vacation_active,
        old,
    })
}

async fn view(conn: &Connection, ins: &Inspect) -> Result<View> {
    let old_entries = match &ins.old {
        Some((_, text)) => match crate::filters::read(text) {
            crate::filters::Reading::Rules(entries) => Some(entries),
            crate::filters::Reading::Edited => None,
        },
        None => None,
    };
    let old_rules = match &old_entries {
        Some(entries) => convert_all(entries, &conn.mailboxes().await?.0),
        None => Vec::new(),
    };
    let kind = match (&ins.chosen, &ins.reading) {
        (None, _) => Kind::None,
        (_, Reading::Empty) => Kind::Empty,
        (_, Reading::Foreign) => Kind::Foreign,
        (_, Reading::Opaque) => Kind::Opaque,
        (_, Reading::Rules(_)) => Kind::Rules,
    };
    let migration_pending = kind == Kind::None && ins.old.is_some() && old_entries.is_some();
    let (rules, external, differs, forward) = match ins.parsed() {
        Some(p) => (p.meta.rules.clone(), p.external.clone(), p.differs, p.meta.vacation_forward.clone()),
        None if migration_pending => (old_rules.clone(), String::new(), false, None),
        None => (Vec::new(), String::new(), false, None),
    };
    let vacation_included = !ins.locked() && ins.include_now();
    let active = match (&ins.chosen, &ins.old) {
        (Some(c), _) => c.is_active,
        (None, Some((o, _))) if migration_pending => o.is_active,
        _ => false,
    };
    let caps = Caps {
        regex: ins.has("regex"),
        body: ins.has("body"),
        mime: ins.has("mime"),
        period: super::period::supports_periods(&ins.extensions),
        mailboxid: ins.has("mailboxid"),
        spamguard: super::generate::supports_spam_guard(&ins.extensions),
        include: ins.has("include"),
        redirect_limit: ins.redirects,
        max_forwards_now: max_forwards(&rules, forward.as_ref()) as u32,
        max_size: ins.max_size,
    };
    Ok(View {
        script_name: ins.chosen.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
        active,
        kind,
        raw_text: ins.text.clone(),
        rules,
        external,
        differs,
        caps,
        vacation_included,
        vacation_conflict: ins.vacation_active && !vacation_included,
        displaced: ins.chosen.as_ref().is_some_and(|c| !c.is_active) && ins.vacation_active,
        old_script_present: ins.old.is_some(),
        old_rules,
        migration_pending,
        old_active: ins.old.as_ref().is_some_and(|(o, _)| o.is_active),
        vacation_forward: forward.is_some_and(|f| f.enabled),
    })
}

/// Reads the filters.
pub async fn load(conn: &Connection) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    Ok(Loaded { view: view(conn, &ins).await?, state: ins.list.state.clone() })
}

/// After a write that went through: read again, once more on failure, and never report a
/// failed save for it.
async fn read_back(conn: &Connection) -> Result<Loaded> {
    match load(conn).await {
        Ok(l) => Ok(l),
        Err(_) => load(conn).await.map_err(|_| Error::temporary(SAVED_NOT_READ)),
    }
}

fn new_id(used: &HashSet<String>) -> Result<String> {
    loop {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).map_err(|e| Error::temporary(format!("random numbers: {e}")))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !used.contains(&id) {
            return Ok(id);
        }
    }
}

pub fn rules_to_json(rules: &[Rule]) -> String {
    J::Arr(rules.iter().map(Rule::to_j).collect()).to_json()
}

pub fn rules_from_json(text: &str) -> Result<Vec<Rule>> {
    let j = J::parse(text).ok_or_else(|| Error::invalid("The rules are not valid JSON."))?;
    let items = j.as_arr().ok_or_else(|| Error::invalid("The rules are a JSON list."))?;
    items.iter().map(|r| Rule::from_j(r).map_err(Error::invalid)).collect()
}

/// Uploads, has the server check, and writes `text` as the filters script.
async fn put(conn: &Connection, ins: &Inspect, text: &str, activate: bool) -> Result<()> {
    if let Some(max) = ins.max_size.filter(|max| text.len() as u64 > *max) {
        return Err(Error::invalid(format!(
            "These filters are too big for the server: the script would be {} bytes and the server takes {max}. \
             Remove or shorten some rules.",
            text.len()
        )));
    }
    let blob_id = conn.sieve_upload(text).await?;
    if let Some(reason) = conn.sieve_validate(&blob_id).await? {
        return Err(Error::invalid(format!("The server does not accept these filters: {reason}")));
    }
    let (name, id) = match &ins.chosen {
        Some(c) => (c.name.as_str(), Some(c.id.as_str())),
        None => (NEW_SCRIPT, None),
    };
    conn.sieve_write(name, id, &blob_id, &ins.list.state, activate).await
}

/// The script runs when it is the active one; a first script is made active, as Bulwark does.
fn activate_after(ins: &Inspect) -> bool {
    ins.chosen.as_ref().is_none_or(|c| c.is_active)
}

async fn save_with(conn: &Connection, ins: &Inspect, rules: Vec<Rule>, activate: bool) -> Result<Loaded> {
    let (mut meta, external, ext_requires) = match ins.parsed() {
        Some(p) => (p.meta.clone(), p.external.clone(), p.external_requires.clone()),
        None if ins.locked() => return Err(Error::invalid(LOCKED)),
        None => (Metadata::default(), String::new(), Vec::new()),
    };
    let mut rules = rules;
    // A rule keeps the keys of its own that this app does not know.
    for rule in &mut rules {
        if let Some(old) = meta.rules.iter().find(|o| o.id == rule.id) {
            for (k, v) in &old.extra {
                if !rule.extra.iter().any(|(rk, _)| rk == k) {
                    rule.extra.push((k.clone(), v.clone()));
                }
            }
        }
    }
    let mut used: HashSet<String> = rules.iter().filter(|r| !r.id.is_empty()).map(|r| r.id.clone()).collect();
    for rule in rules.iter_mut().filter(|r| r.id.is_empty()) {
        rule.id = new_id(&used)?;
        used.insert(rule.id.clone());
    }
    check_rules(&rules, &ins.extensions).map_err(Error::invalid)?;
    let include = ins.has("include") && (meta.include_vacation || ins.vacation_active);
    meta.rules = rules;
    meta.include_vacation = include;
    if !ins.extensions.is_empty() {
        let mailboxid = ins.has("mailboxid");
        for rule in meta.rules.iter().filter(|r| r.enabled) {
            if let Some(missing) = rule_requires(rule, mailboxid, &ins.extensions).into_iter().find(|e| !ins.has(e)) {
                return Err(Error::invalid(format!(
                    "The rule \"{}\" needs the Sieve extension \"{missing}\", which this server does not have.",
                    rule.name.trim()
                )));
            }
        }
        let wanted = bulwark_requires(&meta, include, &ins.extensions);
        if let Some(missing) = wanted.iter().find(|e| !ins.has(e) && !ext_requires.contains(e)) {
            return Err(Error::invalid(format!(
                "These filters need the Sieve extension \"{missing}\", which this server does not have."
            )));
        }
    }
    let text = generate(
        &meta,
        &external,
        &Options { external_requires: &ext_requires, include_vacation: include, extensions: &ins.extensions },
    );
    put(conn, ins, &text, activate).await?;
    read_back(conn).await
}

/// Writes `rules` as the filters. A script the app cannot read as rules is never replaced.
pub async fn save(conn: &Connection, base: Option<&str>, rules: Vec<Rule>) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    let activate = activate_after(&ins);
    save_with(conn, &ins, rules, activate).await
}

/// What a single-rule edit does.
pub enum Edit {
    Toggle(bool),
    Up,
    Down,
    Delete,
}

/// Changes the rule `id` and saves.
pub async fn edit(conn: &Connection, base: Option<&str>, id: &str, how: Edit) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    let mut rules = ins.parsed().map(|p| p.meta.rules.clone()).unwrap_or_default();
    let at = rules.iter().position(|r| r.id == id).ok_or_else(|| Error::invalid("That rule is not there any more."))?;
    match how {
        Edit::Toggle(on) => rules[at].enabled = on,
        Edit::Up if at > 0 => rules.swap(at, at - 1),
        Edit::Down if at + 1 < rules.len() => rules.swap(at, at + 1),
        Edit::Up | Edit::Down => {}
        Edit::Delete => {
            rules.remove(at);
        }
    }
    let activate = activate_after(&ins);
    save_with(conn, &ins, rules, activate).await
}

/// The app's "Use these filters" switch: makes the script the one the server runs, or stops
/// it. Switching on rewrites a script the app can read when the away message needs including.
pub async fn set_enabled(conn: &Connection, base: Option<&str>, on: bool) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    let script = ins.chosen.as_ref().ok_or_else(|| Error::invalid("There are no filters saved yet."))?;
    if on {
        match ins.parsed() {
            Some(p) if ins.include_now() != p.meta.include_vacation => {
                let rules = p.meta.rules.clone();
                return save_with(conn, &ins, rules, true).await;
            }
            _ if script.is_active => {}
            _ => conn.sieve_activate(Some(&script.id), &ins.list.state).await?,
        }
    } else if script.is_active {
        conn.sieve_activate(None, &ins.list.state).await?;
    }
    read_back(conn).await
}

/// Writes `text` as the script as it is (the raw editor): the server checks it first.
pub async fn raw_save(conn: &Connection, base: Option<&str>, text: &str) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    let activate = activate_after(&ins);
    put(conn, &ins, text, activate).await?;
    read_back(conn).await
}

/// Throws the script the app cannot read away and starts with no rules.
pub async fn reset_to_visual(conn: &Connection, base: Option<&str>) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    if !ins.locked() {
        return Err(Error::invalid("The filters can be read as rules; there is nothing to start over."));
    }
    let include = ins.has("include") && ins.vacation_active;
    let meta = Metadata { include_vacation: include, ..Metadata::default() };
    let text = generate(
        &meta,
        "",
        &Options { external_requires: &[], include_vacation: include, extensions: &ins.extensions },
    );
    let activate = activate_after(&ins);
    put(conn, &ins, &text, activate).await?;
    read_back(conn).await
}

/// Deletes the old `kweb-filters` script once the rules live in the new one.
pub async fn migrate_old(conn: &Connection, base: Option<&str>) -> Result<Loaded> {
    let ins = inspect(conn).await?;
    check_state(base, &ins.list.state)?;
    let (old, _) = ins.old.as_ref().ok_or_else(|| Error::invalid("There is no earlier filter script to remove."))?;
    if ins.chosen.is_none() || ins.locked() {
        return Err(Error::invalid("Save the filters first; the earlier script is removed after that."));
    }
    let mut state = ins.list.state.clone();
    if old.is_active {
        conn.sieve_activate(None, &state).await?;
        state = conn.sieve_scripts().await?.state;
    }
    conn.sieve_destroy(&old.id, &state).await?;
    read_back(conn).await
}
