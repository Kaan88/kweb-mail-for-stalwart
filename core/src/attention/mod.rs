//! The attention layer (M12 stage 8): a few plain rules that read the cache after a sync (the
//! agenda, the tasks, the Inbox headers and previews) and say what is worth a look.
//!
//! No model, no network, nothing leaves the phone. The rules are pure functions over [`Input`];
//! [`evaluate`] runs them all. A finding has a stable id; one the user swiped away stays away
//! ([`Dismissed`]); one whose data changed so the rule no longer holds is simply not produced
//! again. Never a shared account, never Spam or Trash, never encrypted mail (its text is not in
//! the cache). The assistant is not involved: a finding only *names* an assistant action
//! ([`ServiceAction`]) that the user may start with a tap.

mod calendar;
mod mail;
pub mod patterns;
mod settings;
pub(crate) mod slots;
mod tasks;

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{MessageHeader, Occurrence};

pub(crate) use settings::{load_dismissed, load_settings, save_dismissed, save_settings};
pub use settings::{AttentionSettings, MailScope, TileSwitches};

/// The thresholds of the rules. Fixed in this release (owner, 2026-11-16).
pub mod limits {
    /// An event this close with no place or link is flagged.
    pub const NO_PLACE_WITHIN_SECS: i64 = 2 * 3600;
    /// A day with this many timed events is busy.
    pub const BUSY_EVENTS: usize = 5;
    /// A day with less free time than this between 09:00 and 18:00 is busy.
    pub const BUSY_FREE_MINUTES: i64 = 30;
    /// Two events at different places closer than this are a tight change.
    pub const TIGHT_CHANGE_MINUTES: i64 = 15;
    /// More tasks than this due today are too many.
    pub const TOO_MANY_TASKS: usize = 5;
    /// A mail is waiting for an answer after this many days.
    pub const WAITING_DAYS: i64 = 2;
    /// An invitation this far ahead that is not answered is flagged.
    pub const INVITATION_DAYS: i64 = 14;
    /// The calendar rules look this many days ahead.
    pub const CALENDAR_DAYS: i64 = 7;
    /// Mail rules look this many days back.
    pub const MAIL_DAYS: i64 = 7;
    /// "Open mails" looks this many days back.
    pub const OPEN_MAILS_DAYS: i64 = 30;
    /// A mail shorter than this (bytes, as the cache has it) can be waiting for an answer.
    pub const SHORT_MAIL_BYTES: u64 = 6000;
}

/// One of the 13 rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
pub enum AttentionRule {
    Overlap,
    NoPlace,
    OpenInvitation,
    BusyDay,
    TightChange,
    Overdue,
    NoTimeToday,
    TooMany,
    MailMayFinish,
    DateInMail,
    DeadlineWords,
    InvitationFile,
    WaitingForYou,
}

/// The settings switch a rule belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
pub enum AttentionFamily {
    Calendar,
    Tasks,
    Mail,
}

/// The Home tile whose sparkle shows a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum HomeTile {
    Unread,
    NextEvent,
    DueToday,
}

/// Where findings are listed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum AttentionPlace {
    Home,
    Calendar,
    Tasks,
    /// The mail findings of one conversation.
    Thread {
        thread_id: String,
    },
    /// The mail findings of every conversation: for the small mark beside a row.
    MailRows,
}

/// A time to offer, Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TimeSlot {
    pub start: i64,
    pub end: i64,
}

/// What an editor is opened with: the same fields as `EventInput`, so the host fills the form.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Prefill {
    pub title: String,
    /// Local "YYYY-MM-DDTHH:MM:SS" ("00:00:00" when `all_day`).
    pub start: String,
    pub all_day: bool,
    pub is_task: bool,
    /// Whole minutes; 0 when the host should use its default.
    pub duration_minutes: u32,
}

/// What the phone itself can do for a finding, on the user's tap. Nothing here reaches the service.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PhoneAction {
    MoveEvent {
        event_id: String,
        recurrence_id: Option<String>,
    },
    /// Show `Finding::slots` as chips; a chip opens the event of `event_id` in the editor at that time.
    FindSlot {
        event_id: String,
        recurrence_id: Option<String>,
    },
    /// The cached mails whose subject shares the event's title.
    OpenMails {
        message_ids: Vec<String>,
    },
    /// Accept, Maybe or Decline (the M5 answer).
    Respond {
        event_id: String,
        recurrence_id: Option<String>,
    },
    /// List the gaps of the day (`Finding::slots`).
    FreeTime,
    /// Open the maps app with both places.
    Directions {
        from: String,
        to: String,
    },
    /// Set these tasks due today (the user's own change, with one Undo).
    MoveToday {
        task_ids: Vec<String>,
    },
    OpenMail {
        message_id: String,
        thread_id: String,
    },
    MarkDone {
        task_id: String,
    },
    AddToCalendar {
        prefill: Prefill,
    },
    AddTask {
        prefill: Prefill,
    },
}

/// What a button with a sparkle would send to the chosen service. Named here, never run here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum ServiceAction {
    LookInMail,
    PlanMyDay,
    CheckTheMail,
    AddDates,
    ReplyIdeas,
}

/// One thing worth a look. The host draws the sentence from the rule and these fields (so it is
/// in the user's language); `names`, `times` and `count` say, per rule:
///
/// - Overlap: names = [the two titles, the later one last], times = [their starts].
/// - NoPlace: names = [title], times = [start], count = mails that share the title.
/// - OpenInvitation: names = [title, the event it overlaps or empty], times = [start].
/// - BusyDay: times = [start of the day], count = timed events, `slots` = the gaps.
/// - TightChange: names = [title A, title B, place A, place B], times = [start A, start B].
/// - Overdue / NoTimeToday / TooMany: names = the first task titles, count = tasks.
/// - MailMayFinish: names = [task title, mail subject], times = [mail received].
/// - DateInMail / DeadlineWords: names = [sender, subject], times = [the date found].
/// - InvitationFile / WaitingForYou: names = [sender, subject], times = [received].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Finding {
    /// Stable: the same data gives the same id; a swipe is remembered under it.
    pub id: String,
    pub rule: AttentionRule,
    pub family: AttentionFamily,
    pub tile: HomeTile,
    pub names: Vec<String>,
    pub times: Vec<i64>,
    pub count: u32,
    /// When it matters (sorting; Unix seconds).
    pub at: i64,
    pub event_ids: Vec<String>,
    pub task_ids: Vec<String>,
    pub message_ids: Vec<String>,
    pub thread_id: Option<String>,
    pub phone: Vec<PhoneAction>,
    pub service: Option<ServiceAction>,
    /// Free times worked out on the phone: for Find a slot and Free time.
    pub slots: Vec<TimeSlot>,
}

impl Finding {
    pub(crate) fn new(rule: AttentionRule, family: AttentionFamily, tile: HomeTile, key: &str) -> Finding {
        let mut h = Sha256::new();
        h.update(format!("{rule:?}|{key}"));
        let digest = h.finalize();
        let id: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        Finding {
            id: format!("{}-{id}", rule_code(rule)),
            rule,
            family,
            tile,
            names: Vec::new(),
            times: Vec::new(),
            count: 0,
            at: 0,
            event_ids: Vec::new(),
            task_ids: Vec::new(),
            message_ids: Vec::new(),
            thread_id: None,
            phone: Vec::new(),
            service: None,
            slots: Vec::new(),
        }
    }
}

fn rule_code(rule: AttentionRule) -> &'static str {
    match rule {
        AttentionRule::Overlap => "ov",
        AttentionRule::NoPlace => "np",
        AttentionRule::OpenInvitation => "oi",
        AttentionRule::BusyDay => "bd",
        AttentionRule::TightChange => "tc",
        AttentionRule::Overdue => "od",
        AttentionRule::NoTimeToday => "nt",
        AttentionRule::TooMany => "tm",
        AttentionRule::MailMayFinish => "mf",
        AttentionRule::DateInMail => "dm",
        AttentionRule::DeadlineWords => "dw",
        AttentionRule::InvitationFile => "if",
        AttentionRule::WaitingForYou => "wy",
    }
}

/// The phone's clock and zone, so the rules work in local time.
#[derive(Clone, Copy)]
pub struct Clock {
    pub now: i64,
    pub zone: Tz,
}

impl Clock {
    pub fn local(&self, ts: i64) -> NaiveDateTime {
        Utc.timestamp_opt(ts, 0).single().map(|d| d.with_timezone(&self.zone).naive_local()).unwrap_or_default()
    }

    /// Unix seconds of a local wall-clock time; the earlier one when it happens twice, the next
    /// valid minute when the clock skips it.
    pub fn ts(&self, local: NaiveDateTime) -> i64 {
        match self.zone.from_local_datetime(&local) {
            chrono::LocalResult::Single(d) => d.timestamp(),
            chrono::LocalResult::Ambiguous(a, _) => a.timestamp(),
            chrono::LocalResult::None => self
                .zone
                .from_local_datetime(&(local + chrono::Duration::hours(1)))
                .earliest()
                .map_or(0, |d| d.timestamp()),
        }
    }

    pub fn now_local(&self) -> NaiveDateTime {
        self.local(self.now)
    }

    /// Unix seconds of local midnight at the start of the day that holds `ts`.
    pub fn day_start(&self, ts: i64) -> i64 {
        self.ts(self.local(ts).date().and_hms_opt(0, 0, 0).unwrap_or_default())
    }

    /// Unix seconds of `hour:00` local on the day that holds `ts`.
    pub fn at_hour(&self, ts: i64, hour: u32) -> i64 {
        self.ts(self.local(ts).date().and_hms_opt(hour, 0, 0).unwrap_or_default())
    }
}

/// Everything the rules read. All of it is the account's own cache.
pub struct Input<'a> {
    pub clock: Clock,
    /// The agenda entries (events and tasks) of the own calendars, any span the cache holds.
    pub entries: &'a [Occurrence],
    /// Mails the mail rules look at: the chosen folders, the last days, never Spam or Trash.
    pub mail: &'a [MessageHeader],
    /// Mails of the last 30 days outside Spam and Trash, to find the ones that mention an event.
    pub recent_mail: &'a [MessageHeader],
    /// The user's first names (identities), for "Ayse asked you".
    pub first_names: &'a [String],
    /// The user's own addresses, lower case.
    pub own_addresses: &'a HashSet<String>,
    /// For a thread id: the newest message the user wrote in it (Unix seconds).
    pub my_last_in_thread: &'a HashMap<String, i64>,
    pub settings: &'a AttentionSettings,
}

/// Runs the rules of the families that are on. The findings are sorted by the time they matter.
pub fn evaluate(input: &Input<'_>) -> Vec<Finding> {
    let mut out = Vec::new();
    if input.settings.calendar {
        calendar::rules(input, &mut out);
    }
    if input.settings.tasks {
        tasks::rules(input, &mut out);
    }
    if input.settings.mail {
        mail::rules(input, &mut out);
    }
    out.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
    out
}

/// A person, not a machine: an address that is not a no-reply or notification sender. The cache
/// holds no List-Id or bulk headers, so this goes by the address only.
pub fn from_a_person(m: &MessageHeader) -> bool {
    let email = m.from_email.to_lowercase();
    let local = email.split('@').next().unwrap_or("");
    const MACHINE: [&str; 14] = [
        "noreply",
        "no-reply",
        "no_reply",
        "donotreply",
        "do-not-reply",
        "notification",
        "notifications",
        "newsletter",
        "mailer-daemon",
        "postmaster",
        "bounce",
        "alerts",
        "marketing",
        "updates",
    ];
    !MACHINE.iter().any(|p| local.contains(p)) && !email.is_empty()
}

#[cfg(test)]
mod tests;
