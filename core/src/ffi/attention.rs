//! The attention layer's calls: the settings, the findings per place, and a swipe. See
//! [`crate::attention`]. Nothing here reaches the assistant or the network except the agenda
//! refresh the host asks for; the findings are worked out from the cache.

use std::collections::{HashMap, HashSet};

use chrono_tz::Tz;

use super::Core;
use crate::{
    assistant,
    attention::{
        self, load_dismissed, load_settings, save_dismissed, save_settings, AttentionPlace, AttentionSettings, Clock,
        Finding, MailScope, ServiceAction,
    },
    model::{AiSettings, MessageHeader},
    Error, Result,
};

/// How far back the agenda is read (overdue tasks) and ahead.
const BACK_DAYS: i64 = 60;
const AHEAD_DAYS: i64 = 15;

/// Whether the assistant is set up so a ✦ button may be offered: on, a key, a model.
fn assistant_ready(s: &AiSettings) -> bool {
    s.enabled && s.has_key && !s.model.trim().is_empty()
}

/// Whether the user's switches let the service do this.
fn allowed(s: &AiSettings, action: ServiceAction) -> bool {
    match action {
        ServiceAction::AddDates | ServiceAction::PlanMyDay => s.allow_extract,
        ServiceAction::ReplyIdeas => s.allow_ideas,
        ServiceAction::LookInMail | ServiceAction::CheckTheMail => s.allow_chat,
    }
}

#[uniffi::export]
impl Core {
    /// Home and attention, per account (defaults when never set).
    pub fn attention_settings(&self) -> Result<AttentionSettings> {
        load_settings(&self.store, &self.account_id()?)
    }

    pub fn attention_set_settings(&self, settings: AttentionSettings) -> Result<()> {
        save_settings(&self.store, &self.account_id()?, &settings)
    }

    /// What is worth a look in `place`, from the cache (nothing is sent anywhere). With `refresh`
    /// the agenda is read from the server first when it can be reached; the cache answers when it
    /// cannot. A finding the user swiped away is not in the list. Without a set-up assistant, or
    /// when the user's switches do not allow an action, a finding has its phone actions only, and
    /// one with none is left out. `time_zone` is the phone's IANA zone and `now` its clock (Unix seconds).
    pub fn attention_findings(
        &self,
        place: AttentionPlace,
        time_zone: String,
        now: i64,
        refresh: bool,
    ) -> Result<Vec<Finding>> {
        let account_id = self.account_id()?;
        let zone: Tz = time_zone.parse().map_err(|_| Error::invalid("unknown time zone"))?;
        let settings = load_settings(&self.store, &account_id)?;
        if !(settings.calendar || settings.tasks || settings.mail) {
            return Ok(Vec::new());
        }
        let (from, to) = (now - BACK_DAYS * 86_400, now + AHEAD_DAYS * 86_400);
        if refresh && (settings.calendar || settings.tasks) {
            // Best effort: the cache is read either way.
            let _ = self.agenda(from, to, time_zone.clone());
        }
        let entries = self.store.agenda_list(&account_id, from, to)?;

        let boxes = self.store.list_mailboxes(&account_id)?;
        let skip: HashSet<&str> = boxes
            .iter()
            .filter(|b| matches!(b.role.as_deref(), Some("junk" | "trash" | "drafts")))
            .map(|b| b.id.as_str())
            .collect();
        let scoped: Vec<&str> = boxes
            .iter()
            .filter(|b| match settings.mail_scope {
                MailScope::Inbox => b.role.as_deref() == Some("inbox"),
                MailScope::InboxAndNotified => b.role.as_deref() == Some("inbox") || b.notify,
            })
            .filter(|b| !skip.contains(b.id.as_str()))
            .map(|b| b.id.as_str())
            .collect();
        let collect = |ids: Vec<&str>, days: i64| -> Result<Vec<MessageHeader>> {
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for id in ids {
                for m in self.store.list_messages(&account_id, id, 300, 0)? {
                    // Never a message that is also in Spam or Trash.
                    if now - m.received_at <= days * 86_400
                        && !m.mailbox_ids.iter().any(|b| skip.contains(b.as_str()))
                        && seen.insert(m.id.clone())
                    {
                        out.push(m);
                    }
                }
            }
            Ok(out)
        };
        let mail =
            if settings.mail || settings.tasks { collect(scoped, attention::limits::MAIL_DAYS)? } else { Vec::new() };
        let recent = if settings.calendar {
            collect(
                boxes.iter().filter(|b| !skip.contains(b.id.as_str())).map(|b| b.id.as_str()).collect(),
                attention::limits::OPEN_MAILS_DAYS,
            )?
        } else {
            Vec::new()
        };

        let identities = self.store.identities_list(&account_id)?;
        let own: HashSet<String> = identities.iter().map(|i| i.email.to_lowercase()).collect();
        let mut first_names: Vec<String> =
            identities.iter().filter_map(|i| i.name.split_whitespace().next().map(str::to_string)).collect();
        first_names.sort();
        first_names.dedup();

        // The newest message the user wrote in each conversation that could be waiting.
        let mut mine: HashMap<String, i64> = HashMap::new();
        if settings.mail {
            for thread in mail
                .iter()
                .filter(|m| now - m.received_at >= attention::limits::WAITING_DAYS * 86_400)
                .map(|m| m.thread_id.clone())
                .collect::<HashSet<_>>()
            {
                let newest = self
                    .store
                    .thread_messages(&account_id, &thread)?
                    .iter()
                    .filter(|m| own.contains(&m.from_email.to_lowercase()))
                    .map(|m| m.received_at)
                    .max();
                if let Some(at) = newest {
                    mine.insert(thread, at);
                }
            }
        }

        let input = attention::Input {
            clock: Clock { now, zone },
            entries: &entries,
            mail: &mail,
            recent_mail: &recent,
            first_names: &first_names,
            own_addresses: &own,
            my_last_in_thread: &mine,
            settings: &settings,
        };
        let dismissed = load_dismissed(&self.store, &account_id)?;
        let ai = assistant::load(&self.store, &*self.secrets, &account_id)?;
        let ready = assistant_ready(&ai);
        let mut list = attention::evaluate(&input);
        list.retain(|f| !dismissed.contains(&f.id));
        for f in &mut list {
            if f.service.is_some_and(|a| !ready || !allowed(&ai, a)) {
                f.service = None;
            }
        }
        // A finding with nothing to offer is not shown.
        list.retain(|f| !f.phone.is_empty() || f.service.is_some());
        list.retain(|f| match &place {
            AttentionPlace::Home => true,
            AttentionPlace::Calendar => f.family == attention::AttentionFamily::Calendar,
            AttentionPlace::Tasks => f.family == attention::AttentionFamily::Tasks,
            AttentionPlace::Thread { thread_id } => {
                f.family == attention::AttentionFamily::Mail && f.thread_id.as_deref() == Some(thread_id.as_str())
            }
            AttentionPlace::MailRows => f.family == attention::AttentionFamily::Mail,
        });
        Ok(list)
    }

    /// The user swiped a finding away: it stays away. (A finding that no longer holds needs nothing.)
    pub fn attention_dismiss(&self, finding_id: String, now: i64) -> Result<()> {
        let account_id = self.account_id()?;
        let mut dismissed = load_dismissed(&self.store, &account_id)?;
        dismissed.add(&finding_id, now);
        save_dismissed(&self.store, &account_id, &dismissed)
    }

    /// Brings every swiped finding back (the settings screen's reset, and tests).
    pub fn attention_reset_dismissed(&self) -> Result<()> {
        self.store.kv_delete(&self.account_id()?, "attention.dismissed")
    }
}
