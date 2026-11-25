//! M12 stage 6 against the mock mail server and the mock AI service: the earlier chats the core
//! keeps per account, and "Catch me up". The service is the mock; no real service was tried.

use std::sync::Arc;

use serde_json::json;

use crate::{
    assistant::{fresh_id, Rig, KEY},
    support::*,
};
use kweb_core::{
    ffi::AiChatListener,
    model::{AiChatTurn, AiSettings, AiStep},
    Error,
};

const ZONE: &str = "Europe/Berlin";
const TODAY: &str = "2026-11-16";

struct Quiet;

impl AiChatListener for Quiet {
    fn text(&self, _piece: String) {}
    fn step(&self, _step: AiStep) {}
}

fn ready() -> Rig {
    let rig = Rig::new();
    rig.save_agreed(AiSettings { allow_chat: true, ..rig.settings() }, Some(KEY.into()));
    rig.core.ai_chat_consent(true).unwrap();
    rig
}

fn say(rig: &Rig, chat: &str, text: &str) -> Result<AiChatTurn, Error> {
    rig.core.ai_chat_send(chat.into(), text.into(), ZONE.into(), fresh_id(), Arc::new(Quiet))
}

fn refused<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    match result {
        Err(Error::Invalid { msg }) => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

// ---- earlier chats --------------------------------------------------------------------------

#[test]
fn a_chat_is_kept_after_each_answer_and_opens_again_with_its_words() {
    let rig = ready();
    rig.core.ai_chat_new("c1".into()).unwrap();
    rig.ai.answer(&["Hello ", "there."]);
    say(&rig, "c1", "  What is\nthe plan?  ").unwrap();
    rig.ai.answer(&["Second answer."]);
    say(&rig, "c1", "And then?").unwrap();

    let list = rig.core.ai_chats().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "c1");
    assert_eq!(list[0].title, "What is", "the first line of the first message");
    assert_eq!(list[0].model, "test-model");
    assert!(list[0].current_service && list[0].service.starts_with("127.0.0.1"));

    // Another conversation takes the place of the current one; the kept one comes back by its id.
    rig.core.ai_chat_new("c2".into()).unwrap();
    let opened = rig.core.ai_chat_open("c1".into()).unwrap();
    let words: Vec<(bool, &str)> = opened.messages.iter().map(|m| (m.from_user, m.text.as_str())).collect();
    assert_eq!(
        words,
        [(true, "What is\nthe plan?"), (false, "Hello there."), (true, "And then?"), (false, "Second answer.")]
    );
    assert!(!opened.service_changed);

    // The next message continues it: the service is told the earlier exchanges.
    rig.ai.answer(&["Third."]);
    say(&rig, "c1", "Thanks").unwrap();
    let sent = rig.ai.last().messages().len();
    assert!(sent >= 6, "system, two earlier exchanges and the new message: {sent}");
    assert_eq!(rig.core.ai_chat_open("c1".into()).unwrap().messages.len(), 6);
    assert_eq!(rig.core.ai_chats().unwrap().len(), 1, "the same chat, not a new one");
}

#[test]
fn only_the_words_are_kept_not_what_the_model_looked_up() {
    let rig = ready();
    let id = rig.mail("Budget", "The secret figure is 40.000 EUR.");
    rig.core.ai_chat_new("c1".into()).unwrap();
    rig.ai.queue(vec![
        Script::Tools {
            text: Vec::new(),
            calls: vec![MockCall { id: "t1".into(), name: "read_message".into(), args: json!({"message_id": id}) }],
        },
        Script::Answer(vec!["It is about the budget.".into()]),
    ]);
    say(&rig, "c1", "What is that mail?").unwrap();
    let opened = rig.core.ai_chat_open("c1".into()).unwrap();
    assert_eq!(opened.messages.len(), 2);
    assert!(opened.messages.iter().all(|m| !m.text.contains("secret figure")));
}

#[test]
fn an_earlier_chat_makes_no_change_by_itself_even_when_automatic_changes_are_switched_on() {
    let rig = ready();
    let id = rig.mail("Invoice", "Due on Friday.");
    rig.core.ai_chat_new("c1".into()).unwrap();
    rig.ai.answer(&["Hi."]);
    say(&rig, "c1", "Hello").unwrap();
    rig.core.ai_chat_new("other".into()).unwrap();
    rig.core.ai_chat_open("c1".into()).unwrap();
    // The switch is off after opening; the user may switch it on again, and still a change that
    // follows mail the model read waits for the tap.
    rig.core.ai_chat_auto("c1".into(), true).unwrap();
    rig.ai.queue(vec![
        Script::Tools {
            text: Vec::new(),
            calls: vec![MockCall { id: "t1".into(), name: "read_message".into(), args: json!({"message_id": id}) }],
        },
        Script::Tools {
            text: Vec::new(),
            calls: vec![MockCall {
                id: "t2".into(),
                name: "mark_messages".into(),
                args: json!({"message_ids": [id], "read": true}),
            }],
        },
        Script::Answer(vec!["Marked.".into()]),
    ]);
    let turn = say(&rig, "c1", "Read the invoice mail and mark it").unwrap();
    assert_eq!(turn.proposals.len(), 1);
    assert!(turn.automatic.is_empty(), "nothing was applied without a tap");
    rig.sync();
    assert!(!rig.server.is_read(&id));
}

#[test]
fn a_chat_held_with_another_model_says_so_and_the_next_message_needs_that_models_consent() {
    let rig = ready();
    rig.core.ai_chat_new("c1".into()).unwrap();
    rig.ai.answer(&["Hi."]);
    say(&rig, "c1", "Hello").unwrap();

    let other = AiSettings { model: "other-model".into(), allow_chat: true, ..rig.settings() };
    rig.core.set_ai_settings(other.clone(), None).unwrap();
    let list = rig.core.ai_chats().unwrap();
    assert!(!list[0].current_service);
    let opened = rig.core.ai_chat_open("c1".into()).unwrap();
    assert!(opened.service_changed);
    assert_eq!(opened.model, "test-model");
    // No consent for the new model yet: nothing is sent.
    let before = rig.ai.requests();
    refused(say(&rig, "c1", "Continue"));
    assert_eq!(rig.ai.requests(), before);
    // After the user's yes for it the chat goes on and is now held with the new model.
    rig.core.set_ai_settings(other, None).unwrap();
    rig.core.ai_chat_consent(true).unwrap();
    rig.ai.answer(&["Continued."]);
    say(&rig, "c1", "Continue").unwrap();
    assert!(rig.core.ai_chats().unwrap()[0].current_service);
}

#[test]
fn one_chat_or_all_can_be_deleted_and_a_reset_removes_them() {
    let rig = ready();
    for chat in ["a", "b", "c"] {
        rig.core.ai_chat_new(chat.into()).unwrap();
        rig.ai.answer(&["ok"]);
        say(&rig, chat, &format!("message {chat}")).unwrap();
    }
    assert_eq!(rig.core.ai_chats().unwrap().len(), 3);
    assert_eq!(rig.core.ai_chats().unwrap()[0].id, "c", "the newest first");
    // The current one is "c": deleting it ends the conversation too.
    rig.core.ai_chat_delete("c".into()).unwrap();
    refused(say(&rig, "c", "still there?"));
    assert_eq!(rig.core.ai_chats().unwrap().len(), 2);
    refused(rig.core.ai_chat_open("c".into()));
    assert_eq!(rig.core.ai_chats_delete_all().unwrap(), 2);
    assert!(rig.core.ai_chats().unwrap().is_empty());

    rig.core.ai_chat_new("d".into()).unwrap();
    rig.ai.answer(&["ok"]);
    say(&rig, "d", "again").unwrap();
    assert_eq!(rig.core.ai_chats().unwrap().len(), 1);
    rig.core.ai_reset_settings().unwrap();
    assert!(rig.core.ai_chats().unwrap().is_empty(), "the reset clears the earlier chats");
}

#[test]
fn a_failed_send_keeps_nothing() {
    let rig = ready();
    rig.core.ai_chat_new("c1".into()).unwrap();
    rig.ai.set(Script::Status(500));
    assert!(say(&rig, "c1", "Hello").is_err());
    assert!(rig.core.ai_chats().unwrap().is_empty());
}

// ---- Catch me up ----------------------------------------------------------------------------

/// The number the request gives the mail with this subject.
fn number_of(request: &str, subject: &str) -> usize {
    let at = request.find(&format!("<subject>{subject}</subject>")).unwrap_or_else(|| panic!("{subject} not sent"));
    let head = &request[..at];
    let open = head.rfind("<number>").unwrap() + "<number>".len();
    head[open..head.rfind("</number>").unwrap()].parse().unwrap()
}

fn catchup_rig() -> Rig {
    let rig = Rig::new();
    rig.save_agreed(rig.settings(), Some(KEY.into()));
    rig
}

#[test]
fn a_catch_up_sends_only_the_start_of_unread_mail_and_groups_the_answer_by_sender() {
    let rig = catchup_rig();
    rig.server.deliver_at(
        "Maria",
        "maria@example.test",
        "Quarterly numbers",
        "Can you send the figures by Friday?",
        state_now() - 60,
    );
    rig.server.deliver_at("Maria", "maria@example.test", "Lunch", "Tomorrow at one?", state_now() - 120);
    rig.server.deliver_at("News", "news@example.test", "Weekly digest", "Ten things <b>to read</b>", state_now() - 30);
    rig.sync();

    let preview = rig.core.ai_catchup_preview(TODAY.into(), ZONE.into()).unwrap();
    assert!(preview.messages >= 3 && !preview.reads_mail);
    assert!(preview.text.contains("Can you send the figures by Friday?"));
    assert!(preview.text.contains("Ten things &lt;b&gt;to read&lt;/b&gt;"), "escaped");
    assert_eq!(rig.ai.requests(), 0, "a preview sends nothing");

    let (q, l, d) = (
        number_of(&preview.text, "Quarterly numbers"),
        number_of(&preview.text, "Lunch"),
        number_of(&preview.text, "Weekly digest"),
    );
    let answer = format!(
        "ITEM | {q} | ANSWER | Asks for the figures by Friday\nITEM | {d} | INFO | A newsletter\nITEM | {l} | ANSWER | Asks about lunch tomorrow\nITEM | 999 | INFO | not a mail"
    );
    rig.ai.answer(&[answer.as_str()]);
    let since = rig.core.ai_catchup_since().unwrap();
    let result = rig.core.ai_catchup_run(TODAY.into(), ZONE.into(), fresh_id()).unwrap();
    assert_eq!(result.mails, preview.messages);
    assert!((result.since - since).abs() <= 2);
    let maria = result.groups.iter().find(|g| g.sender == "Maria").expect("a group for Maria");
    assert_eq!(maria.items.len(), 2, "two mails of one sender are one group");
    assert!(maria.items.iter().all(|i| i.needs_answer));
    assert!(maria
        .items
        .iter()
        .any(|i| i.message.subject == "Quarterly numbers" && i.line == "Asks for the figures by Friday"));
    let news = result.groups.iter().find(|g| g.sender == "News").unwrap();
    assert!(!news.items[0].needs_answer);
    assert!(result.groups.iter().flat_map(|g| &g.items).all(|i| !i.message.id.is_empty()));

    // What the service was sent: the start of each mail and no body.
    assert_eq!(rig.ai.requests(), 1);
    assert!(rig.ai.last().system().contains("DATA"));
    // The look-back moved: the same mails are not caught up again.
    assert!(rig.core.ai_catchup_since().unwrap() > since);
    assert!(rig.core.ai_catchup_preview(TODAY.into(), ZONE.into()).is_err(), "nothing new since");
}

#[test]
fn a_catch_up_needs_the_consent_and_the_summaries_switch_and_an_unreadable_answer_moves_nothing() {
    let rig = catchup_rig();
    rig.server.deliver_at("Maria", "maria@example.test", "Hello", "Hi there", state_now() - 10);
    rig.sync();
    let since = rig.core.ai_catchup_since().unwrap();

    rig.ai.answer(&["I could not do that."]);
    match rig.core.ai_catchup_run(TODAY.into(), ZONE.into(), fresh_id()) {
        Err(Error::Temporary { msg }) => assert!(msg.contains("could not be read"), "{msg}"),
        other => panic!("expected a temporary error, got {other:?}"),
    }
    assert!((rig.core.ai_catchup_since().unwrap() - since).abs() <= 2, "an unreadable answer moves nothing");

    // Summaries off: refused before anything is sent.
    let off = AiSettings { allow_summaries: false, ..rig.settings() };
    rig.core.set_ai_settings(off.clone(), None).unwrap();
    let before = rig.ai.requests();
    refused(rig.core.ai_catchup_run(TODAY.into(), ZONE.into(), fresh_id()));
    assert_eq!(rig.ai.requests(), before);
    // Not agreed to (a changed model clears the yes): refused too.
    rig.core.set_ai_settings(AiSettings { model: "x".into(), allow_summaries: true, ..off }, None).unwrap();
    refused(rig.core.ai_catchup_run(TODAY.into(), ZONE.into(), fresh_id()));
    assert_eq!(rig.ai.requests(), before);
}

fn state_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}
