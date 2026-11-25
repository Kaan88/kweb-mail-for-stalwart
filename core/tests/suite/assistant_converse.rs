//! `provider::converse` against the mock AI service: tool calls streamed and whole, the retry
//! without tools, and what is sent back of an earlier turn. Every service speaks the OpenAI
//! protocol. Written by the package core-provider.
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
        provider::{self, Endpoint, Timeouts, Usage},
        wire::{Reply, ToolCall, ToolResult, ToolSpec, ToolsRefused, Turn},
    },
    model::AiEffort,
    Error,
};
use serde_json::{json, Value};

const TIMEOUTS: Timeouts =
    Timeouts { connect: Duration::from_secs(5), first: Duration::from_secs(5), idle: Duration::from_secs(5) };

type Turned = Result<(Reply, ToolsRefused), Error>;

/// What the mock streams as reasoning with every answer, and the core sends back with the calls.
const REASONING: &str = "private reasoning";

fn endpoint(rig: &Rig) -> Endpoint {
    Endpoint { base_url: rig.ai.base_url(), model: "m".into(), effort: AiEffort::Default, key: Some(KEY.into()) }
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

/// The results of `calls()`: one hit, and a failure.
fn results() -> Turn {
    Turn::Results(vec![
        ToolResult { id: "c1".into(), name: "search_mail".into(), content: "one hit".into(), is_error: false },
        ToolResult { id: "c2".into(), name: "list_folders".into(), content: "failed".into(), is_error: true },
    ])
}

/// Nothing of the key or of what the mock writes into its errors.
fn leaks_nothing(message: &str) {
    for secret in [KEY, "sk-test", "Incorrect", "Credentials", "does not support", "bad key", "Bearer"] {
        assert!(!message.contains(secret), "{message}");
    }
}

#[test]
fn a_turn_with_tool_calls_is_read_from_a_stream() {
    let rig = Rig::new();
    for split in [false, true] {
        rig.ai.split_events(split);
        let ep = endpoint(&rig);
        rig.ai.queue(vec![Script::Tools { text: vec!["Let me ".into(), "look.".into()], calls: mock_calls() }]);
        let (result, heard) = converse(&rig, &ep, &question(), &tools());
        let (reply, refused) = result.unwrap();
        assert_eq!(refused, ToolsRefused::No);
        assert_eq!(reply.text, "Let me look.", "split {split}");
        assert_eq!(heard, "Let me look.");
        assert_eq!(reply.calls, calls(), "split {split}");
        // What the service sent beside the calls and wants back: the reasoning, never shown.
        assert_eq!(reply.raw, Some(json!({"reasoning_content": REASONING})));
        assert!(!heard.contains(REASONING));

        // What left the device: the question, the tools, and the key in its own header only.
        let seen = rig.ai.last();
        assert_eq!(seen.path, "/v1/chat/completions");
        assert_eq!(seen.authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));
        assert_eq!(seen.api_key, None);
        assert_eq!(seen.tool_names(), ["search_mail", "list_folders"]);
        assert_eq!(seen.system(), "sys");
        assert_eq!(seen.user(), "Any mail from Mar\u{ed}a?");
        assert!(!seen.body.to_string().contains(KEY));
        // The tool descriptions carry no `additionalProperties`, and none is without what it needs.
        for tool in seen.body["tools"].as_array().unwrap() {
            assert_eq!(tool["type"], "function");
            assert!(tool["function"]["parameters"].get("additionalProperties").is_none(), "{tool}");
        }

        // The next request carries the turn and its results back.
        let mut turns = question();
        turns.push(Turn::Assistant { text: reply.text, calls: reply.calls, raw: reply.raw });
        turns.push(results());
        rig.ai.answer(&["One, ", "from today."]);
        let (result, heard) = converse(&rig, &ep, &turns, &tools());
        let (reply, refused) = result.unwrap();
        assert_eq!((reply.text.as_str(), reply.calls.len(), refused), ("One, from today.", 0, ToolsRefused::No));
        assert_eq!(heard, "One, from today.");
        let seen = rig.ai.last();
        assert_eq!(seen.tool_results(), [("c1".to_string(), "one hit".to_string()), ("c2".into(), "failed".into())]);
        let messages = seen.messages();
        assert_eq!(messages.len(), 5, "system, user, assistant, two results");
        assert_eq!(messages[2]["content"], "Let me look.");
        assert_eq!(messages[2]["tool_calls"][0]["function"]["name"], "search_mail");
        assert_eq!(
            serde_json::from_str::<Value>(messages[2]["tool_calls"][0]["function"]["arguments"].as_str().unwrap())
                .unwrap(),
            json!({"query": "mar\u{ed}a \u{1f600}"})
        );
        assert_eq!(messages[2]["reasoning_content"], REASONING, "the reasoning goes back with its calls");
        assert_eq!((messages[3]["role"].as_str(), messages[3]["tool_call_id"].as_str()), (Some("tool"), Some("c1")));
        assert_eq!((messages[4]["role"].as_str(), messages[4]["content"].as_str()), (Some("tool"), Some("failed")));
    }
}

#[test]
fn a_turn_of_calls_alone_is_an_answer() {
    let rig = Rig::new();
    rig.ai.queue(vec![Script::Tools { text: Vec::new(), calls: mock_calls() }]);
    let (result, heard) = converse(&rig, &endpoint(&rig), &question(), &tools());
    let (reply, _) = result.unwrap();
    assert_eq!((reply.text.as_str(), heard.as_str()), ("", ""));
    assert_eq!(reply.calls, calls());
}

#[test]
fn a_turn_of_calls_alone_goes_back_without_content_and_a_user_message_after_results_follows_an_assistant_turn() {
    let rig = Rig::new();
    let mut turns = question();
    turns.push(Turn::Assistant { text: String::new(), calls: calls(), raw: None });
    turns.push(results());
    turns.push(Turn::User("And from Pedro?".into()));
    rig.ai.answer(&["No."]);
    converse(&rig, &endpoint(&rig), &turns, &tools()).0.unwrap();

    let messages = rig.ai.last().messages();
    let roles: Vec<&str> = messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool", "tool", "assistant", "user"]);
    // No text: the content is null, never an empty text some services refuse.
    assert!(messages[2]["content"].is_null());
    assert_eq!(messages[2]["tool_calls"].as_array().unwrap().len(), 2);
    assert!(messages.iter().all(|m| m["content"] != ""));
    // A user message may not follow tool results directly: the run before ended, and it says so.
    assert_eq!(messages[5]["content"], provider::ENDED);
    assert_eq!(messages[6]["content"], "And from Pedro?");
    assert!(!rig.ai.last().body.to_string().contains("additionalProperties"));
}

#[test]
fn an_answer_in_one_document_is_read() {
    let rig = Rig::new();
    rig.ai.set(Script::Whole("  Nothing new.".into()));
    let (result, heard) = converse(&rig, &endpoint(&rig), &question(), &tools());
    let (reply, refused) = result.unwrap();
    assert_eq!(refused, ToolsRefused::No);
    assert_eq!(reply.text, "Nothing new.");
    assert_eq!(heard, "Nothing new.");
    assert!(reply.calls.is_empty());
    assert_eq!(reply.raw, None);
}

#[test]
fn a_plain_streamed_answer_needs_no_tools_and_sends_no_tools_key() {
    let rig = Rig::new();
    rig.ai.answer(&["Hello ", "there."]);
    let (result, heard) = converse(&rig, &endpoint(&rig), &question(), &[]);
    let (reply, refused) = result.unwrap();
    assert_eq!((reply.text.as_str(), heard.as_str(), refused), ("Hello there.", "Hello there.", ToolsRefused::No));
    assert!(rig.ai.last().body.get("tools").is_none());
    assert!(rig.ai.last().tool_names().is_empty());
}

#[test]
fn a_service_that_takes_no_tools_is_asked_once_more_without_them() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
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
fn the_retry_without_tools_sends_the_earlier_calls_and_results_as_plain_text() {
    let rig = Rig::new();
    let mut turns = question();
    turns.push(Turn::Assistant {
        text: "Let me look.".into(),
        calls: calls(),
        raw: Some(json!({"reasoning_content": REASONING})),
    });
    turns.push(results());
    rig.ai.set(Script::NoTools(vec!["Only one hit.".into()]));
    let (result, _) = converse(&rig, &endpoint(&rig), &turns, &tools());
    let (reply, refused) = result.unwrap();
    assert_eq!((reply.text.as_str(), refused), ("Only one hit.", ToolsRefused::Yes));

    let seen = rig.ai.seen();
    assert_eq!(seen.len(), 2);
    // The first request, with tools, carried the calls and the results as such.
    assert_eq!(seen[0].tool_names().len(), 2);
    assert_eq!(seen[0].tool_results().len(), 2);
    assert!(seen[0].messages()[2]["tool_calls"].is_array());
    // The second has none of that: no tools, no call, no tool message, no id of a call.
    let body = seen[1].body.to_string();
    assert!(seen[1].body.get("tools").is_none());
    for word in ["tool_calls", "tool_call_id", "reasoning_content", "\"role\":\"tool\""] {
        assert!(!body.contains(word), "{word} in {body}");
    }
    let messages = seen[1].messages();
    let roles: Vec<&str> = messages.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
    assert_eq!(messages[2]["content"], "Let me look.");
    // The results are in a user message, each framed as data with the tool's name.
    let framed = messages[3]["content"].as_str().unwrap();
    assert!(framed.contains("<tool_result name=\"search_mail\">\none hit\n</tool_result>"), "{framed}");
    assert!(framed.contains("<tool_result name=\"list_folders\">\nfailed\n</tool_result>"), "{framed}");
}

#[test]
fn a_refusal_that_is_not_about_tools_or_a_second_one_is_the_status() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
    // The body does not mention tools: no second request.
    rig.ai.set(Script::Status(400));
    let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
    assert_eq!(message, "the service did not accept the request (status 400)");
    leaks_nothing(&message);
    assert_eq!(rig.ai.requests(), 1);

    // It does, but the request without tools fails too: one retry, then the first status.
    rig.ai.queue(vec![Script::NoTools(vec!["never".into()]), Script::Status(500)]);
    let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
    assert_eq!(message, "the service has a problem (status 500); try again later");
    leaks_nothing(&message);
    assert_eq!(rig.ai.requests(), 3);
}

#[test]
fn only_a_body_that_says_the_model_takes_no_tools_counts_as_no_tools() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
    // A refusal that is about the tools in another way (a description it did not take, a part of the
    // conversation it did not like): a fixed sentence, no second request, never "no tools".
    for body in [
        r#"{"error":{"message":"Invalid schema for function 'search_mail': 'properties' is required"}}"#,
        r#"{"error":{"message":"Invalid value for tool_call_id in messages[3]"}}"#,
        r#"[{"error":{"code":400,"message":"Function call is missing a thought_signature"}}]"#,
    ] {
        let before = rig.ai.requests();
        rig.ai.set(Script::StatusBody(400, body.into()));
        let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
        assert_eq!(message, provider::TOOLS_NOT_ACCEPTED, "{body}");
        leaks_nothing(&message);
        assert_eq!(rig.ai.requests(), before + 1, "{body}");
    }
    // The model wrote a call the service could not read.
    rig.ai.set(Script::StatusBody(
        400,
        r#"{"error":{"code":"tool_use_failed","message":"Failed to call a function"}}"#.into(),
    ));
    let message = temporary(converse(&rig, &ep, &question(), &tools()).0);
    assert_eq!(message, provider::BROKEN_CALL);
    leaks_nothing(&message);
    // A 422 that says tools are not supported (Mistral's status) is a model without tools.
    let before = rig.ai.requests();
    rig.ai.queue(vec![
        Script::StatusBody(422, "tools are not supported by this model".into()),
        Script::Answer(vec!["From memory.".into()]),
    ]);
    let (reply, refused) = converse(&rig, &ep, &question(), &tools()).0.unwrap();
    assert_eq!((reply.text.as_str(), refused), ("From memory.", ToolsRefused::Yes));
    assert_eq!(rig.ai.requests(), before + 2);
}

#[test]
fn a_model_that_takes_no_functions_at_all_is_its_own_error_and_says_nothing_else() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
    // "Functions" is as good as "tools": the request is tried once more without them, is refused
    // again, and the fact is all that comes out.
    rig.ai.set(Script::NoFunctions);
    let result = converse(&rig, &ep, &question(), &tools()).0;
    let Err(error) = result else { panic!("an answer from a model that takes nothing") };
    assert_eq!(error, Error::ToolsUnsupported);
    // The core's own words, none of the service's (which echo the key).
    assert_eq!(error.to_string(), "the model does not support tools");
    for text in [error.to_string(), format!("{error:?}")] {
        for secret in [KEY, "sk-test", "Credentials", "Functions are", "Bearer"] {
            assert!(!text.contains(secret), "{text}");
        }
    }
    assert_eq!(rig.ai.requests(), 2);

    // Without tools there is nothing to blame them for: the status, one request.
    let message = temporary(converse(&rig, &ep, &question(), &[]).0);
    assert_eq!(message, "the service did not accept the request (status 400)");
    assert_eq!(rig.ai.requests(), 3);
}

#[test]
fn asking_for_the_token_counts_is_tried_once_more_without_when_the_service_refuses_the_option() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
    let run = |rig: &Rig| {
        let mut heard = String::new();
        let result = rig.rt.block_on(provider::converse_with_usage(
            &ep,
            "sys",
            &question(),
            &[],
            TIMEOUTS,
            &mut |p| heard.push_str(p),
            true,
        ));
        (result, heard)
    };
    // A service that takes the option reports the counts.
    rig.ai.answer(&["Hi."]);
    let (result, heard) = run(&rig);
    let (reply, refused, usage) = result.unwrap();
    assert_eq!((reply.text.as_str(), heard.as_str(), refused), ("Hi.", "Hi.", ToolsRefused::No));
    assert_eq!(usage, Usage { input: Some(USAGE_IN), output: Some(USAGE_OUT) });
    assert_eq!(rig.ai.last().body["stream_options"]["include_usage"], true);
    assert_eq!(rig.ai.requests(), 1);

    // Mistral answers 422 to it: once more without it, same endpoint, and no counts.
    rig.ai.set_vendor(Vendor::Mistral);
    let (result, heard) = run(&rig);
    let (reply, refused, usage) = result.unwrap();
    assert_eq!((reply.text.as_str(), heard.as_str(), refused), ("Hi.", "Hi.", ToolsRefused::No));
    assert_eq!(usage, Usage::default());
    let seen = rig.ai.seen();
    assert_eq!(seen.len(), 3);
    assert!(seen[1].refused && seen[1].body.get("stream_options").is_some());
    assert!(!seen[2].refused && seen[2].body.get("stream_options").is_none());
    assert_eq!(seen[2].authorization.as_deref(), Some(format!("Bearer {KEY}").as_str()));

    // The same for a single answer.
    let (answer, usage) =
        rig.rt.block_on(provider::complete_with_usage(&ep, "sys", "usr", TIMEOUTS, &mut |_| {}, true)).unwrap();
    assert_eq!((answer.as_str(), usage), ("Hi.", Usage::default()));
    let seen = rig.ai.seen();
    assert_eq!(seen.len(), 5);
    assert!(seen[3].refused && !seen[4].refused && seen[4].body.get("stream_options").is_none());
}

#[test]
fn neither_the_key_nor_the_services_words_come_out_through_an_error() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
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
    // Each failure in the core's own words.
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
    rig.ai.set(Script::Redirect(format!("{}/chat/completions", elsewhere.base_url())));
    let message = temporary(converse(&rig, &endpoint(&rig), &question(), &tools()).0);
    assert!(message.contains("status 307"), "{message}");
    assert_eq!(elsewhere.requests(), 0);
    assert_eq!(rig.ai.requests(), 1);
}

#[test]
fn a_service_that_goes_quiet_is_given_up_on() {
    let rig = Rig::new();
    let timeouts = Timeouts { idle: Duration::from_millis(300), ..TIMEOUTS };
    rig.ai.set(Script::Hang(vec!["start".into()]));
    let started = Instant::now();
    let (result, heard) = converse_within(&rig, &endpoint(&rig), &question(), &tools(), timeouts);
    assert_eq!(temporary(result), "the service stopped answering");
    assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    assert_eq!(heard, "start");
    // Before the first word the longer wait counts.
    rig.ai.set(Script::Hang(Vec::new()));
    let timeouts = Timeouts { first: Duration::from_millis(900), idle: Duration::from_millis(100), ..TIMEOUTS };
    let started = Instant::now();
    let ep = endpoint(&rig);
    assert_eq!(
        temporary(converse_within(&rig, &ep, &question(), &tools(), timeouts).0),
        "the service stopped answering"
    );
    assert!(started.elapsed() >= Duration::from_millis(800), "{:?}", started.elapsed());
}

#[test]
fn a_bad_address_sends_nothing_and_a_missing_key_sends_no_authorization() {
    let rig = Rig::new();
    let bad = Endpoint { base_url: "ftp://example.test/v1".into(), ..endpoint(&rig) };
    assert!(matches!(converse(&rig, &bad, &question(), &tools()).0, Err(Error::Invalid { .. })));
    assert_eq!(rig.ai.requests(), 0);
    // Without a key no authorization is sent at all, and no x-api-key either.
    let open = Endpoint { key: None, ..endpoint(&rig) };
    converse(&rig, &open, &question(), &tools()).0.unwrap();
    assert_eq!((rig.ai.last().authorization, rig.ai.last().api_key), (None, None));
}

#[test]
fn queued_scripts_answer_one_request_each_and_then_the_set_one_does() {
    let rig = Rig::new();
    let ep = endpoint(&rig);
    rig.ai.answer(&["set"]);
    rig.ai.queue(vec![Script::Answer(vec!["first".into()]), Script::Answer(vec!["second".into()])]);
    let texts: Vec<String> = (0..4).map(|_| converse(&rig, &ep, &question(), &[]).0.unwrap().0.text).collect();
    assert_eq!(texts, ["first", "second", "set", "set"]);
}
