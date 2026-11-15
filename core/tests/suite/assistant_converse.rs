//! `provider::converse` against the mock AI service: tool calls in both protocols, streamed and
//! whole, and the retry without tools. Written by the package core-provider.
//!
//! The rig is the one of the stage 1 suite: `crate::assistant::Rig` (with `eventually`, `KEY`
//! and the rest beside it).

use std::time::{Duration, Instant};

use crate::{
    assistant::{Rig, KEY},
    support::*,
};
use kweb_core::{
    assistant::{
        provider::{self, Endpoint, Timeouts},
        wire::{Reply, ToolCall, ToolResult, ToolSpec, ToolsRefused, Turn},
    },
    model::{AiEffort, AiProvider},
    Error,
};
use serde_json::{json, Value};

const BOTH: [AiProvider; 2] = [AiProvider::OpenAiCompatible, AiProvider::Anthropic];

const TIMEOUTS: Timeouts =
    Timeouts { connect: Duration::from_secs(5), first: Duration::from_secs(5), idle: Duration::from_secs(5) };

type Turned = Result<(Reply, ToolsRefused), Error>;

fn endpoint(rig: &Rig, provider: AiProvider) -> Endpoint {
    Endpoint {
        provider,
        base_url: rig.ai.base_url(),
        model: "m".into(),
        effort: AiEffort::Default,
        key: Some(KEY.into()),
    }
}

fn tools() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "search_mail",
            description: "Searches the mail.",
            schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        },
        ToolSpec { name: "list_folders", description: "Lists the folders.", schema: json!({"type": "object"}) },
    ]
}

fn question() -> Vec<Turn> {
    vec![Turn::User("Any mail from Mar\u{ed}a?".into())]
}

/// One turn, with the given timeouts; also what the listener heard.
fn converse_within(
    rig: &Rig,
    ep: &Endpoint,
    turns: &[Turn],
    tools: &[ToolSpec],
    timeouts: Timeouts,
) -> (Turned, String) {
    let mut heard = String::new();
    let result = rig.rt.block_on(provider::converse(ep, "sys", turns, tools, timeouts, &mut |p| heard.push_str(p)));
    (result, heard)
}

fn converse(rig: &Rig, ep: &Endpoint, turns: &[Turn], tools: &[ToolSpec]) -> (Turned, String) {
    converse_within(rig, ep, turns, tools, TIMEOUTS)
}

fn temporary(result: Turned) -> String {
    match result {
        Err(Error::Temporary { msg }) => msg,
        other => panic!("expected Error::Temporary, got {other:?}"),
    }
}

fn mock_calls() -> Vec<MockCall> {
    vec![
        MockCall { id: "c1".into(), name: "search_mail".into(), args: json!({"query": "mar\u{ed}a \u{1f600}"}) },
        MockCall { id: "c2".into(), name: "list_folders".into(), args: json!({}) },
    ]
}

fn calls() -> Vec<ToolCall> {
    mock_calls().into_iter().map(|c| ToolCall { id: c.id, name: c.name, args: c.args }).collect()
}

/// Nothing of the key or of what the mock writes into its errors.
fn leaks_nothing(message: &str) {
    for secret in [KEY, "sk-test", "Incorrect", "Credentials", "does not support", "bad key", "Bearer"] {
        assert!(!message.contains(secret), "{message}");
    }
}

#[test]
fn a_turn_with_tool_calls_is_read_from_a_stream_in_both_protocols() {
    let rig = Rig::new();
    for split in [false, true] {
        rig.ai.split_events(split);
        for provider in BOTH {
            let ep = endpoint(&rig, provider);
            rig.ai.queue(vec![Script::Tools { text: vec!["Let me ".into(), "look.".into()], calls: mock_calls() }]);
            let (result, heard) = converse(&rig, &ep, &question(), &tools());
            let (reply, refused) = result.unwrap();
            assert_eq!(refused, ToolsRefused::No);
            assert_eq!(reply.text, "Let me look.", "{provider:?} split {split}");
            assert_eq!(heard, "Let me look.");
            assert_eq!(reply.calls, calls(), "{provider:?} split {split}");

            // What left the device: the question, the tools, and the key in its own header only.
            let seen = rig.ai.last();
            assert_eq!(seen.tool_names(), ["search_mail", "list_folders"]);
            assert_eq!(seen.system(), "sys");
            assert_eq!(seen.user(), "Any mail from Mar\u{ed}a?");
            assert!(!seen.body.to_string().contains(KEY));
            match provider {
                AiProvider::OpenAiCompatible => {
                    assert_eq!(seen.path, "/v1/chat/completions");
                    assert_eq!(seen.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
                    assert_eq!(seen.api_key, None);
                    assert_eq!(reply.raw, None);
                }
                AiProvider::Anthropic => {
                    assert_eq!(seen.path, "/v1/messages");
                    assert_eq!(seen.api_key.as_deref(), Some(KEY));
                    assert_eq!(seen.authorization, None);
                    assert_eq!(seen.version.as_deref(), Some(provider::ANTHROPIC_VERSION));
                    // The service's blocks, the thinking with its signature among them.
                    assert_eq!(
                        reply.raw,
                        Some(json!([
                            {"type": "thinking", "thinking": "private reasoning", "signature": "mock-signature"},
                            {"type": "text", "text": "Let me look."},
                            {"type": "tool_use", "id": "c1", "name": "search_mail",
                             "input": {"query": "mar\u{ed}a \u{1f600}"}},
                            {"type": "tool_use", "id": "c2", "name": "list_folders", "input": {}}]))
                    );
                }
            }

            // The next request carries the turn and its results back.
            let mut turns = question();
            turns.push(Turn::Assistant { text: reply.text, calls: reply.calls, raw: reply.raw.clone() });
            turns.push(Turn::Results(vec![
                ToolResult { id: "c1".into(), name: "search_mail".into(), content: "one hit".into(), is_error: false },
                ToolResult { id: "c2".into(), name: "list_folders".into(), content: "failed".into(), is_error: true },
            ]));
            rig.ai.answer(&["One, ", "from today."]);
            let (result, heard) = converse(&rig, &ep, &turns, &tools());
            let (reply, refused) = result.unwrap();
            assert_eq!((reply.text.as_str(), reply.calls.len(), refused), ("One, from today.", 0, ToolsRefused::No));
            assert_eq!(heard, "One, from today.");
            let seen = rig.ai.last();
            assert_eq!(
                seen.tool_results(),
                [("c1".to_string(), "one hit".to_string()), ("c2".into(), "failed".into())]
            );
            let messages = seen.messages();
            match provider {
                AiProvider::OpenAiCompatible => {
                    assert_eq!(messages.len(), 5, "system, user, assistant, two results");
                    assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "search_mail");
                    assert_eq!(
                        serde_json::from_str::<Value>(
                            messages[2]["tool_calls"][0]["function"]["arguments"].as_str().unwrap()
                        )
                        .unwrap(),
                        json!({"query": "mar\u{ed}a \u{1f600}"})
                    );
                }
                AiProvider::Anthropic => {
                    assert_eq!(messages.len(), 3, "user, assistant, one message of results");
                    assert_eq!(Some(&messages[1]["content"]), turns_raw(&turns), "sent back unchanged");
                    assert_eq!(messages[2]["content"][1]["is_error"], true);
                }
            }
        }
    }
}

/// The service's blocks of the assistant turn in `turns`.
fn turns_raw(turns: &[Turn]) -> Option<&Value> {
    turns.iter().find_map(|turn| match turn {
        Turn::Assistant { raw, .. } => raw.as_ref(),
        _ => None,
    })
}

#[test]
fn a_turn_of_calls_alone_is_an_answer() {
    let rig = Rig::new();
    for provider in BOTH {
        rig.ai.queue(vec![Script::Tools { text: Vec::new(), calls: mock_calls() }]);
        let (result, heard) = converse(&rig, &endpoint(&rig, provider), &question(), &tools());
        let (reply, _) = result.unwrap();
        assert_eq!((reply.text.as_str(), heard.as_str()), ("", ""), "{provider:?}");
        assert_eq!(reply.calls, calls(), "{provider:?}");
    }
}

#[test]
fn an_answer_in_one_document_is_read_in_both_protocols() {
    let rig = Rig::new();
    for provider in BOTH {
        rig.ai.set(Script::Whole("  Nothing new.".into()));
        let (result, heard) = converse(&rig, &endpoint(&rig, provider), &question(), &tools());
        let (reply, refused) = result.unwrap();
        assert_eq!(refused, ToolsRefused::No);
        assert_eq!(reply.text, "Nothing new.", "{provider:?}");
        assert_eq!(heard, "Nothing new.");
        assert!(reply.calls.is_empty());
        match provider {
            AiProvider::OpenAiCompatible => assert_eq!(reply.raw, None),
            AiProvider::Anthropic => assert_eq!(
                reply.raw,
                Some(json!([{"type": "thinking", "thinking": "private"}, {"type": "text", "text": "  Nothing new."}]))
            ),
        }
    }
}

#[test]
fn a_plain_streamed_answer_needs_no_tools_and_sends_no_tools_key() {
    let rig = Rig::new();
    for provider in BOTH {
        rig.ai.answer(&["Hello ", "there."]);
        let (result, heard) = converse(&rig, &endpoint(&rig, provider), &question(), &[]);
        let (reply, refused) = result.unwrap();
        assert_eq!((reply.text.as_str(), heard.as_str(), refused), ("Hello there.", "Hello there.", ToolsRefused::No));
        assert!(rig.ai.last().body.get("tools").is_none(), "{provider:?}");
        assert!(rig.ai.last().tool_names().is_empty());
    }
}

#[test]
fn a_service_that_takes_no_tools_is_asked_once_more_without_them() {
    let rig = Rig::new();
    let ep = endpoint(&rig, AiProvider::OpenAiCompatible);
    rig.ai.set(Script::NoTools(vec!["From what ".into(), "you told me.".into()]));
    let (result, heard) = converse(&rig, &ep, &question(), &tools());
    let (reply, refused) = result.unwrap();
    assert_eq!(refused, ToolsRefused::Yes);
    assert_eq!(reply, Reply { text: "From what you told me.".into(), calls: Vec::new(), raw: None });
    assert_eq!(heard, "From what you told me.", "nothing of the refusal was heard");

    // Exactly two requests, to the same place with the same key, the second without tools.
    let seen = rig.ai.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(rig.ai.requests(), 2);
    assert_eq!(seen[0].tool_names(), ["search_mail", "list_folders"]);
    assert!(seen[1].body.get("tools").is_none());
    for request in &seen {
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
    }
    assert_eq!(seen[0].messages(), seen[1].messages());

    // A request that carries no tools is not refused, and is sent once.
    let (result, _) = converse(&rig, &ep, &question(), &[]);
    assert_eq!(result.unwrap().1, ToolsRefused::No);
    assert_eq!(rig.ai.requests(), 3);
}

#[test]
fn a_refusal_that_is_not_about_tools_or_a_second_one_is_the_status() {
    let rig = Rig::new();
    let ep = endpoint(&rig, AiProvider::OpenAiCompatible);
    // The body does not mention tools: no second request.
    rig.ai.set(Script::Status(400));
    let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
    assert_eq!(message, "the service did not accept the request (status 400)");
    leaks_nothing(&message);
    assert_eq!(rig.ai.requests(), 1);

    // It does, but the request without tools fails too: one retry, then the first status.
    rig.ai.queue(vec![Script::NoTools(vec!["never".into()]), Script::Status(500)]);
    let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
    assert_eq!(message, "the service did not accept the request (status 400)");
    leaks_nothing(&message);
    assert_eq!(rig.ai.requests(), 3);

    // The other protocol has no such rule: its 400 is final, whatever it says.
    rig.ai.set(Script::NoTools(vec!["never".into()]));
    let message = temporary(converse(&rig, &endpoint(&rig, AiProvider::Anthropic), &question(), &tools()).0);
    assert_eq!(message, "the service did not accept the request (status 400)");
    leaks_nothing(&message);
    assert_eq!(rig.ai.requests(), 4);
}

#[test]
fn neither_the_key_nor_the_services_words_come_out_through_an_error() {
    let rig = Rig::new();
    for provider in BOTH {
        let ep = endpoint(&rig, provider);
        for script in [
            Script::Status(400),
            Script::Status(401),
            Script::Status(404),
            Script::Status(429),
            Script::Status(500),
            Script::NoTools(Vec::new()),
            Script::StreamError(vec!["Half".into()]),
            Script::Cut(vec!["Half".into()]),
            Script::Length(vec!["Half".into()]),
            Script::Answer(Vec::new()),
        ] {
            rig.ai.set(script.clone());
            let result = converse(&rig, &ep, &question(), &tools()).0;
            let Err(error) = result else { panic!("{script:?} gave an answer") };
            assert!(matches!(error, Error::Temporary { .. }), "{script:?}: {error:?}");
            leaks_nothing(&error.to_string());
            leaks_nothing(&format!("{error:?}"));
        }
    }
    // Each failure in the core's own words.
    let ep = endpoint(&rig, AiProvider::OpenAiCompatible);
    for (script, words) in [
        (Script::Status(401), "the service refused the key"),
        (Script::StreamError(vec!["Half".into()]), "the service reported an error while answering"),
        (Script::Cut(vec!["Half".into()]), "the connection to the service broke while it was answering"),
        (Script::Length(vec!["Half".into()]), "the answer was cut off by the service's length limit"),
        (Script::Answer(Vec::new()), "the service sent an empty answer"),
    ] {
        rig.ai.set(script);
        assert_eq!(temporary(converse(&rig, &ep, &question(), &tools()).0), words);
    }
}

#[test]
fn a_redirect_is_not_followed_so_the_key_goes_nowhere_else() {
    let rig = Rig::new();
    let elsewhere = rig.rt.block_on(MockAi::start());
    for (provider, path) in [(AiProvider::OpenAiCompatible, "chat/completions"), (AiProvider::Anthropic, "messages")] {
        rig.ai.set(Script::Redirect(format!("{}/{path}", elsewhere.base_url())));
        let message = temporary(converse(&rig, &endpoint(&rig, provider), &question(), &tools()).0);
        assert!(message.contains("status 307"), "{message}");
    }
    assert_eq!(elsewhere.requests(), 0);
    assert_eq!(rig.ai.requests(), 2);
}

#[test]
fn a_service_that_goes_quiet_is_given_up_on() {
    let rig = Rig::new();
    let timeouts = Timeouts { idle: Duration::from_millis(300), ..TIMEOUTS };
    for provider in BOTH {
        rig.ai.set(Script::Hang(vec!["start".into()]));
        let started = Instant::now();
        let (result, heard) = converse_within(&rig, &endpoint(&rig, provider), &question(), &tools(), timeouts);
        assert_eq!(temporary(result), "the service stopped answering", "{provider:?}");
        assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
        assert_eq!(heard, "start");
    }
    // Before the first word the longer wait counts.
    rig.ai.set(Script::Hang(Vec::new()));
    let timeouts = Timeouts { first: Duration::from_millis(900), idle: Duration::from_millis(100), ..TIMEOUTS };
    let started = Instant::now();
    let ep = endpoint(&rig, AiProvider::OpenAiCompatible);
    assert_eq!(
        temporary(converse_within(&rig, &ep, &question(), &tools(), timeouts).0),
        "the service stopped answering"
    );
    assert!(started.elapsed() >= Duration::from_millis(800), "{:?}", started.elapsed());
}

#[test]
fn anthropic_without_a_key_and_a_bad_address_send_nothing() {
    let rig = Rig::new();
    let no_key = Endpoint { key: None, ..endpoint(&rig, AiProvider::Anthropic) };
    let result = converse(&rig, &no_key, &question(), &tools()).0;
    assert!(matches!(&result, Err(Error::Invalid { msg }) if msg == "this service needs a key"), "{result:?}");
    let bad = Endpoint { base_url: "ftp://example.test/v1".into(), ..endpoint(&rig, AiProvider::OpenAiCompatible) };
    assert!(matches!(converse(&rig, &bad, &question(), &tools()).0, Err(Error::Invalid { .. })));
    assert_eq!(rig.ai.requests(), 0);
    // Without a key the other protocol sends no authorization at all.
    let open = Endpoint { key: None, ..endpoint(&rig, AiProvider::OpenAiCompatible) };
    converse(&rig, &open, &question(), &tools()).0.unwrap();
    assert_eq!(rig.ai.last().authorization, None);
}

#[test]
fn queued_scripts_answer_one_request_each_and_then_the_set_one_does() {
    let rig = Rig::new();
    let ep = endpoint(&rig, AiProvider::OpenAiCompatible);
    rig.ai.answer(&["set"]);
    rig.ai.queue(vec![Script::Answer(vec!["first".into()]), Script::Answer(vec!["second".into()])]);
    let texts: Vec<String> = (0..4).map(|_| converse(&rig, &ep, &question(), &[]).0.unwrap().0.text).collect();
    assert_eq!(texts, ["first", "second", "set", "set"]);
}
