//! What the provider and the chat loop pass each other: the turns of a conversation with tools,
//! in a form that fits both protocols. Pure data. This file is the contract between
//! `provider::converse` and `chat::send`; neither side changes it.

use serde_json::Value;

/// A tool as the model is told about it. `schema` is the JSON Schema of its arguments (an object).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// A tool the model wants used. `args` is always a JSON object (`{}` when the model sent
/// something else).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// What a call gave: `id` and `name` of the call it answers, the text for the model, and whether
/// it is an error.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

/// One turn of the conversation as it is sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    User(String),
    /// `raw`: the service's own content blocks (Anthropic), sent back unchanged so thinking blocks survive.
    Assistant {
        text: String,
        calls: Vec<ToolCall>,
        raw: Option<Value>,
    },
    /// The results of the calls of the assistant turn before it, all of them, in order.
    Results(Vec<ToolResult>),
}

/// What the model said in one turn. A turn with calls may have no text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub raw: Option<Value>,
}

/// Whether the service refused the tools, so that the turn was asked again without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolsRefused {
    No,
    Yes,
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Characters this turn costs in a request (text, arguments as JSON, result contents).
pub fn turn_chars(turn: &Turn) -> usize {
    match turn {
        Turn::User(text) => chars(text),
        Turn::Assistant { text, calls, raw } => {
            let calls: usize = calls.iter().map(|c| chars(&c.id) + chars(&c.name) + chars(&c.args.to_string())).sum();
            // Raw blocks are what is really sent when they are there; they hold the text and the
            // calls again, plus what the service wants back unchanged.
            match raw {
                Some(raw) => chars(&raw.to_string()).max(chars(text) + calls),
                None => chars(text) + calls,
            }
        }
        Turn::Results(results) => results.iter().map(|r| chars(&r.id) + chars(&r.name) + chars(&r.content)).sum(),
    }
}

/// Characters the tool list costs in a request: names, descriptions and schemas as JSON.
pub fn specs_chars(tools: &[ToolSpec]) -> usize {
    tools.iter().map(|t| chars(t.name) + chars(t.description) + chars(&t.schema.to_string())).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_turn_costs_its_text_arguments_and_results() {
        assert_eq!(turn_chars(&Turn::User("h\u{e9}llo".into())), 5);
        let call = ToolCall { id: "c1".into(), name: "read".into(), args: json!({"id": "m1"}) };
        let plain = Turn::Assistant { text: "ok".into(), calls: vec![call.clone()], raw: None };
        assert_eq!(turn_chars(&plain), 2 + 2 + 4 + r#"{"id":"m1"}"#.len());
        // Raw blocks count when they are longer than the parts.
        let raw = json!([{"type": "thinking", "thinking": "x".repeat(500), "signature": "s"}]);
        let with_raw = Turn::Assistant { text: "ok".into(), calls: vec![call], raw: Some(raw.clone()) };
        assert_eq!(turn_chars(&with_raw), raw.to_string().chars().count());
        let results = Turn::Results(vec![
            ToolResult { id: "c1".into(), name: "read".into(), content: "abc".into(), is_error: false },
            ToolResult { id: "c2".into(), name: "list".into(), content: String::new(), is_error: true },
        ]);
        assert_eq!(turn_chars(&results), (2 + 4 + 3) + (2 + 4));
    }

    #[test]
    fn the_tool_list_costs_its_names_descriptions_and_schemas() {
        assert_eq!(specs_chars(&[]), 0);
        let spec = ToolSpec { name: "read", description: "Reads.", schema: json!({"type": "object"}) };
        assert_eq!(specs_chars(&[spec.clone(), spec]), 2 * (4 + 6 + r#"{"type":"object"}"#.len()));
    }
}
