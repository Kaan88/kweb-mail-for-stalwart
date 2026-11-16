//! M12 stage 6, "Catch me up" across the bridge: a one-shot assistant run over the unread Inbox mail
//! that came since the last catch-up. [`Core::ai_catchup_preview`] says what would be sent, and
//! [`Core::ai_catchup_run`] sends it, after the user's yes (the host asks as it does for the other
//! runs: the first use for a service and model is the agreement itself, and "Ask before each run"
//! asks again, because the run sends mail).
//!
//! What goes out is the sender, the subject, the date and the first lines of at most 30 mails (the
//! preview the cache keeps; no body, no attachment). The run makes no change and holds no tool: its
//! answer is data, read strictly (`assistant::catchup::parse`), and the link from a line to a mail is
//! made here from the account's own cache. The time of the last catch-up is kept per account and
//! moves only when a run came to a result.

use std::collections::HashMap;

use super::{ai_chat::cancelled, now, Core};
use crate::{
    assistant::{
        self,
        catchup::{self, Chosen},
        changes::message_ref,
        provider::{self, Usage},
    },
    jmap,
    model::{AiCatchUp, AiCatchUpGroup, AiCatchUpItem, AiSparklePreview, AiTask, AiUsage},
    Error, Result,
};

/// Where the time of the last catch-up is kept (Unix seconds, in `kv`).
const LAST_KEY: &str = "catchup_last";
/// The longest name of a run.
const RUN_NAME_CHARS: usize = 100;

/// What a run is made ready as: the mails, the request, and where it looks back to.
struct Prep {
    chosen: Chosen,
    prepared: assistant::request::Prepared,
    since: i64,
}

#[uniffi::export]
impl Core {
    /// When the next catch-up looks back to: the end of the last one, else a day ago (Unix seconds).
    /// Reads the device only.
    pub fn ai_catchup_since(&self) -> Result<i64> {
        let account_id = self.account_id()?;
        self.catchup_since(&account_id)
    }

    /// What a catch-up would send, without sending anything: the host, the model, how many mails, who
    /// they are from and the request itself (`text`), made by the very code the run uses. Invalid when
    /// no service is set up or there is no unread mail since the last catch-up. `today` is the phone's
    /// date "YYYY-MM-DD" and `time_zone` its IANA zone. Reads the cache only.
    pub fn ai_catchup_preview(&self, today: String, time_zone: String) -> Result<AiSparklePreview> {
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        if settings.base_url.is_empty() {
            return Err(Error::invalid("no service is set up for the assistant yet"));
        }
        let host = assistant::settings::host_label(&assistant::settings::check_base_url(&settings.base_url)?);
        let prep = self.catchup_prepare(&account_id, &today, &time_zone)?;
        let senders = prep
            .chosen
            .mails
            .iter()
            .map(|m| {
                crate::compose::rules::address_text(&crate::model::Address {
                    name: m.from_name.clone(),
                    email: m.from_email.clone(),
                })
            })
            .collect();
        Ok(AiSparklePreview {
            host,
            model: settings.model,
            messages: prep.prepared.messages,
            events: 0,
            tasks: 0,
            characters: prep.prepared.characters,
            is_cut: prep.prepared.is_cut || prep.chosen.more > 0,
            senders,
            reads_mail: false,
            text: prep.prepared.user,
        })
    }

    /// Runs a catch-up: sends the request of [`Core::ai_catchup_preview`], reads the answer strictly
    /// and returns the mails grouped by sender, each with its sentence and whether it needs an answer.
    /// Blocks (one run at a time, the assistant's one slot); [`Core::ai_cancel`] with the same `run_id`
    /// stops it (Invalid "cancelled"). Refused before anything is sent, with Invalid: the assistant is
    /// off or not agreed to for this service and model, or summaries are switched off. An answer that
    /// does not fit is `Error::Temporary` "the answer could not be read" and does not move the time of
    /// the last catch-up. No change is made by it.
    pub fn ai_catchup_run(&self, today: String, time_zone: String, run_id: String) -> Result<AiCatchUp> {
        if run_id.trim().is_empty() || run_id.chars().count() > RUN_NAME_CHARS {
            return Err(Error::invalid("the run needs a name"));
        }
        // The slot first: from here on `ai_cancel` and `sign_out` see this run.
        let running = self.ai.begin(&run_id)?;
        let account_id = self.account_id()?;
        let settings = assistant::load(&self.store, &*self.secrets, &account_id)?;
        assistant::check_allowed(&settings, &AiTask::SummarizeThread { thread_id: String::new() })?;
        let prep = self.catchup_prepare(&account_id, &today, &time_zone)?;
        let endpoint = assistant::endpoint(&settings, &*self.secrets)?;
        let host = assistant::settings::host_label(&assistant::settings::check_base_url(&settings.base_url)?);
        let system = assistant::prompts::with_instructions(&prep.prepared.system, &settings.instructions);
        let mut quiet = |_: &str| {};
        let (answer, usage) = self.rt.block_on(async {
            tokio::select! {
                biased;
                () = running.cancel.notified() => Err(cancelled()),
                done = provider::complete_with_usage(
                    &endpoint, &system, &prep.prepared.user, provider::TIMEOUTS, &mut quiet, true,
                ) => done,
            }
        })?;
        let lines = catchup::parse(&answer, prep.chosen.mails.len())?;

        // Each line is tied to a mail of this account's own cache; the groups follow the sender, in
        // the order the newest mail of each stands.
        let folders = self.store.list_mailboxes(&account_id)?;
        let by_number: HashMap<usize, &catchup::Line> = lines.iter().map(|l| (l.number, l)).collect();
        let mut groups: Vec<AiCatchUpGroup> = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        for (at, mail) in prep.chosen.mails.iter().enumerate() {
            let Some(line) = by_number.get(&(at + 1)) else { continue };
            let key = mail.from_email.trim().to_lowercase();
            let name = match mail.from_name.trim() {
                "" => mail.from_email.clone(),
                name => name.to_string(),
            };
            let item = AiCatchUpItem {
                message: message_ref(mail, &folders),
                line: line.text.clone(),
                needs_answer: line.needs_answer,
            };
            match keys.iter().position(|k| *k == key) {
                Some(i) => groups[i].items.push(item),
                None => {
                    keys.push(key);
                    groups.push(AiCatchUpGroup { sender: name, items: vec![item] });
                }
            }
        }
        // The look-back moves only now, when a result was made.
        self.store.kv_set(&account_id, LAST_KEY, &now().to_string())?;
        Ok(AiCatchUp {
            groups,
            mails: prep.chosen.mails.len() as u32,
            more: prep.chosen.more as u32,
            since: prep.since,
            usage: usage_of(usage),
            host,
            model: settings.model,
            characters: prep.prepared.characters,
        })
    }
}

fn usage_of(usage: Usage) -> Option<AiUsage> {
    (usage.input.is_some() || usage.output.is_some())
        .then_some(AiUsage { input_tokens: usage.input, output_tokens: usage.output })
}

impl Core {
    fn catchup_since(&self, account_id: &str) -> Result<i64> {
        let last = self.store.kv_get(account_id, LAST_KEY)?.and_then(|t| t.parse::<i64>().ok());
        Ok(last.unwrap_or_else(|| now() - catchup::FIRST_LOOK_BACK))
    }

    /// The unread Inbox mail since the last catch-up, from the cache, and the request made of it.
    fn catchup_prepare(&self, account_id: &str, today: &str, time_zone: &str) -> Result<Prep> {
        jmap::check_span(0, 1, time_zone)?;
        if !assistant::insights::is_day(today) {
            return Err(Error::invalid("today's date must be written like 2026-11-15"));
        }
        let inbox = self
            .store
            .mailbox_by_role(account_id, "inbox")?
            .ok_or_else(|| Error::invalid("the account has no Inbox"))?;
        let since = self.catchup_since(account_id)?;
        // The newest unread mails: the list is newest first, and 200 is far more than a catch-up takes.
        let headers = self.store.list_messages(account_id, &inbox.id, 200, 0)?;
        let chosen = catchup::choose(&headers, since);
        let prepared = catchup::build(&chosen.mails, today, time_zone)?;
        Ok(Prep { chosen, prepared, since })
    }
}
