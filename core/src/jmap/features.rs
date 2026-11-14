//! Plain JMAP requests for the M10 mail features: the unsubscribe headers, keywords (labels) and
//! search snippets. What Stalwart 1.1.0 does here was found against the real server
//! (`dev/notes/stalwart-mail-features.md`): keywords are ASCII, lower-cased by the server, at
//! most 128 bytes; `List-Unsubscribe` can only be asked for as `:asURLs` (`:asText` fails the
//! whole request); a snippet's text is HTML-escaped with `<mark>` tags.

use serde_json::{json, Value};

use super::{
    send::{pointer_segment, response, updated},
    Connection, MAIL_CAPABILITY,
};
use crate::{Error, Result};

const MESSAGE_GONE: &str = "That message is no longer on the server.";
const URLS_PROPERTY: &str = "header:List-Unsubscribe:asURLs";
const POST_PROPERTY: &str = "header:List-Unsubscribe-Post:asText";

/// One entry of a `SearchSnippet/get` answer, as the server's marked text: `(message id,
/// subject, preview)`.
pub type RawSnippet = (String, Option<String>, Option<String>);

impl Connection {
    /// The addresses of the message's List-Unsubscribe header (brackets taken off, in the order
    /// the header has them) and the text of its List-Unsubscribe-Post header. A message the
    /// server no longer has is `Error::Invalid`.
    pub async fn unsubscribe_headers(&self, message_id: &str) -> Result<(Vec<String>, Option<String>)> {
        let args = json!({
            "accountId": self.account_id(),
            "ids": [message_id],
            "properties": [URLS_PROPERTY, POST_PROPERTY],
        });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/get", args)]).await?;
        let get = response(&responses, "Email/get")?;
        let Some(email) = get["list"].get(0) else { return Err(Error::invalid(MESSAGE_GONE)) };
        Ok(headers_from(email))
    }

    /// Puts the keyword on the message or takes it off (a patch of that one keyword, so no other
    /// keyword is touched). A message the server no longer has is `Error::Invalid`, and so is a
    /// keyword the server refuses.
    pub async fn apply_label(&self, message_id: &str, keyword: &str, on: bool) -> Result<()> {
        let patch =
            json!({ format!("keywords/{}", pointer_segment(keyword)): if on { json!(true) } else { Value::Null } });
        let args = json!({ "accountId": self.account_id(), "update": { message_id: patch } });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/set", args)]).await?;
        updated(response(&responses, "Email/set")?, message_id, MESSAGE_GONE)
    }

    /// Ids of up to `limit` messages that carry the keyword, newest first, in every mailbox.
    pub async fn keyword_ids(&self, keyword: &str, limit: usize) -> Result<Vec<String>> {
        let args = json!({
            "accountId": self.account_id(),
            "filter": { "hasKeyword": keyword },
            "sort": [{ "property": "receivedAt", "isAscending": false }],
            "limit": limit,
        });
        let responses = self.call(&[MAIL_CAPABILITY], &[("Email/query", args)]).await?;
        let ids = response(&responses, "Email/query")?["ids"].as_array().cloned().unwrap_or_default();
        Ok(ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
    }

    /// Where `query` is found in each of the messages, as the server marks it. In batches of
    /// what one request may ask for. A message with no hit has `None` for both.
    pub async fn search_snippets(&self, query: &str, ids: &[String]) -> Result<Vec<RawSnippet>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(self.get_batch()) {
            let args = json!({ "accountId": self.account_id(), "emailIds": chunk, "filter": { "text": query } });
            let responses = self.call(&[MAIL_CAPABILITY], &[("SearchSnippet/get", args)]).await?;
            out.extend(snippets_from(response(&responses, "SearchSnippet/get")?));
        }
        Ok(out)
    }
}

/// The two header values out of an `Email/get` entry.
fn headers_from(email: &Value) -> (Vec<String>, Option<String>) {
    let urls = email[URLS_PROPERTY]
        .as_array()
        .map(|list| list.iter().filter_map(|u| u.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    (urls, email[POST_PROPERTY].as_str().map(str::to_string))
}

fn snippets_from(answer: &Value) -> Vec<RawSnippet> {
    let text = |v: &Value| v.as_str().map(str::to_string);
    answer["list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some((s["emailId"].as_str()?.to_string(), text(&s["subject"]), text(&s["preview"]))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headers_are_read_from_the_answer() {
        let both = json!({"id":"m","header:List-Unsubscribe:asURLs":["https://a.org/u","mailto:u@a.org"],
            "header:List-Unsubscribe-Post:asText":"List-Unsubscribe=One-Click"});
        assert_eq!(
            headers_from(&both),
            (
                vec!["https://a.org/u".to_string(), "mailto:u@a.org".to_string()],
                Some("List-Unsubscribe=One-Click".into())
            )
        );
        let none = json!({"id":"m","header:List-Unsubscribe:asURLs":null,"header:List-Unsubscribe-Post:asText":null});
        assert_eq!(headers_from(&none), (Vec::new(), None));
        assert_eq!(headers_from(&json!({"id":"m"})), (Vec::new(), None));
    }

    #[test]
    fn snippets_are_read_with_nulls_kept_as_none() {
        let answer = json!({"list":[
            {"emailId":"a","subject":"x <mark>y</mark>","preview":null},
            {"emailId":"b","subject":null,"preview":null},
            {"subject":"no id"}], "notFound":["z"]});
        assert_eq!(
            snippets_from(&answer),
            vec![("a".to_string(), Some("x <mark>y</mark>".to_string()), None), ("b".to_string(), None, None)]
        );
        assert!(snippets_from(&json!({"list":null})).is_empty());
    }
}
