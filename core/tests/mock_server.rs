//! Self-test of the mock JMAP server in `support`. It uses jmap-client and reqwest directly,
//! so a failure here is a problem with the mock, not with kweb-core.

mod support;

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::StreamExt;
use jmap_client::{
    client::{Client, Credentials},
    core::{
        query::{Filter, QueryResponse},
        response::{EmailGetResponse, MailboxGetResponse},
    },
    email::{self, query::Comparator},
    event_source::PushNotification,
    mailbox, DataType, Error,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use support::*;

const T: Duration = Duration::from_secs(5);

async fn connect(server: &MockServer, token: &str) -> Result<Client, Error> {
    Client::new().credentials(Credentials::bearer(token)).connect(&server.url()).await
}

async fn signed_in(server: &MockServer) -> Client {
    connect(server, &server.issue_access_token()).await.expect("connect")
}

fn is_status(e: &Error, code: &str) -> bool {
    matches!(e, Error::Server(s) if s.starts_with(code))
}

async fn inbox_id(client: &Client) -> String {
    let mut req = client.build();
    req.query_mailbox().filter(mailbox::query::Filter::role(mailbox::Role::Inbox));
    req.send_single::<QueryResponse>().await.unwrap().take_ids().pop().unwrap()
}

async fn unread_ids(client: &Client, inbox: &str) -> Vec<String> {
    let mut req = client.build();
    req.query_email()
        .filter(Filter::and([email::query::Filter::in_mailbox(inbox), email::query::Filter::not_keyword("$seen")]))
        .sort([Comparator::received_at().descending()]);
    req.send_single::<QueryResponse>().await.unwrap().take_ids()
}

async fn email_query_state(client: &Client) -> String {
    let mut req = client.build();
    req.query_email();
    req.send_single::<QueryResponse>().await.unwrap().query_state().to_string()
}

async fn mailbox_state(client: &Client) -> String {
    let mut req = client.build();
    req.get_mailbox().ids(["mb1"]).properties([mailbox::Property::Id]);
    req.send_single::<MailboxGetResponse>().await.unwrap().state().to_string()
}

fn pkce() -> (String, String) {
    let verifier = "a-test-verifier-that-is-long-enough-to-pass-for-a-real-one-0123456789".to_string();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

async fn body_json(resp: reqwest::Response) -> Value {
    serde_json::from_slice(&resp.bytes().await.unwrap()).unwrap_or(Value::Null)
}

/// reqwest's `json` feature is not enabled for this crate, so build JSON requests by hand.
fn post_json(client: &reqwest::Client, url: impl reqwest::IntoUrl, body: &Value) -> reqwest::RequestBuilder {
    client.post(url).header("content-type", "application/json").body(body.to_string())
}

async fn post_form(url: &str, pairs: &[(&str, &str)]) -> (u16, Value) {
    let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
    let resp = reqwest::Client::new()
        .post(url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, body_json(resp).await)
}

#[tokio::test]
async fn oauth_round_trip_with_pkce_and_rotation() {
    let server = MockServer::start().await;
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

    let meta =
        body_json(http.get(format!("{}/.well-known/oauth-authorization-server", server.url())).send().await.unwrap())
            .await;
    for key in ["authorization_endpoint", "token_endpoint", "registration_endpoint", "revocation_endpoint"] {
        assert!(meta[key].as_str().unwrap().starts_with(&server.url()), "{key}");
    }

    let reg = body_json(
        post_json(
            &http,
            meta["registration_endpoint"].as_str().unwrap(),
            &json!({ "client_name": "test", "redirect_uris": ["http://127.0.0.1/callback"], "token_endpoint_auth_method": "none" }),
        )
        .send()
        .await
        .unwrap(),
    )
    .await;
    let client_id = reg["client_id"].as_str().unwrap().to_string();

    let (verifier, challenge) = pkce();
    let authorize_url = format!(
        "{}?response_type=code&client_id={client_id}&redirect_uri={}&state=xyz&code_challenge={challenge}&code_challenge_method=S256",
        meta["authorization_endpoint"].as_str().unwrap(),
        url::form_urlencoded::byte_serialize(b"http://127.0.0.1:40123/callback").collect::<String>(),
    );

    // Over HTTP: a 302 to the redirect_uri with code and state.
    let resp = http.get(&authorize_url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 302);
    let location = url::Url::parse(resp.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(location.port(), Some(40123));
    let q: std::collections::HashMap<_, _> = location.query_pairs().into_owned().collect();
    assert_eq!(q["state"], "xyz");
    let code = q["code"].clone();

    // The wrong verifier is refused, and burns the code.
    let token_url = meta["token_endpoint"].as_str().unwrap().to_string();
    let exchange = |code: &str, verifier: &str| {
        let (code, verifier, client_id, token_url) =
            (code.to_string(), verifier.to_string(), client_id.clone(), token_url.clone());
        async move {
            post_form(
                &token_url,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("code_verifier", &verifier),
                    ("client_id", &client_id),
                    ("redirect_uri", "http://127.0.0.1:40123/callback"),
                ],
            )
            .await
        }
    };
    let (status, body) = exchange(&code, "not-the-verifier").await;
    assert_eq!((status, body["error"].as_str()), (400, Some("invalid_grant")));
    let (status, _) = exchange(&code, &verifier).await;
    assert_eq!(status, 400, "a code works once");

    // The helper does the same without HTTP.
    let callback = server.authorize(&authorize_url).unwrap();
    let code2 = url::Url::parse(&callback).unwrap().query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
    let (status, tokens) = exchange(&code2, &verifier).await;
    assert_eq!(status, 200, "{tokens}");
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    assert_eq!(tokens["username"], USERNAME);

    // The token works against JMAP.
    connect(&server, tokens["access_token"].as_str().unwrap()).await.expect("access token accepted");

    // Rotation: each refresh returns a new refresh token and kills the old one.
    let rt1 = tokens["refresh_token"].as_str().unwrap().to_string();
    let (status, second) =
        post_form(&token_url, &[("grant_type", "refresh_token"), ("refresh_token", &rt1), ("client_id", &client_id)])
            .await;
    assert_eq!(status, 200);
    let rt2 = second["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(rt1, rt2);
    let (status, body) = post_form(&token_url, &[("grant_type", "refresh_token"), ("refresh_token", &rt1)]).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("invalid_grant")));

    // Revoking the grant makes the newest refresh token fail as well.
    server.revoke_grant();
    let (status, body) = post_form(&token_url, &[("grant_type", "refresh_token"), ("refresh_token", &rt2)]).await;
    assert_eq!((status, body["error"].as_str()), (400, Some("invalid_grant")));
    assert_eq!(server.token_requests(), 6);

    // /revoke answers 200 and retires the token.
    let (_, third) = {
        let callback = server.authorize(&authorize_url).unwrap();
        let code = url::Url::parse(&callback).unwrap().query_pairs().find(|(k, _)| k == "code").unwrap().1.into_owned();
        exchange(&code, &verifier).await
    };
    let rt3 = third["refresh_token"].as_str().unwrap().to_string();
    let (status, _) = post_form(meta["revocation_endpoint"].as_str().unwrap(), &[("token", &rt3)]).await;
    assert_eq!(status, 200);
    let (status, _) = post_form(&token_url, &[("grant_type", "refresh_token"), ("refresh_token", &rt3)]).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn authorize_refuses_unknown_client_and_missing_pkce() {
    let server = MockServer::start().await;
    assert!(server
        .authorize("http://x/authorize?client_id=nope&redirect_uri=http://127.0.0.1/cb&response_type=code")
        .is_err());
    let reg = body_json(
        post_json(
            &reqwest::Client::new(),
            format!("{}/register", server.url()),
            &json!({ "redirect_uris": ["http://127.0.0.1/callback"] }),
        )
        .send()
        .await
        .unwrap(),
    )
    .await;
    let id = reg["client_id"].as_str().unwrap();
    // No PKCE: the redirect carries an error instead of a code.
    let back = server
        .authorize(&format!(
            "http://x/authorize?client_id={id}&redirect_uri=http://127.0.0.1:1/callback&response_type=code&state=s"
        ))
        .unwrap();
    assert!(back.contains("error=invalid_request") && !back.contains("code="), "{back}");
    // A redirect that was not registered is not followed.
    assert!(server
        .authorize(&format!(
            "http://x/authorize?client_id={id}&redirect_uri=http://evil.test/callback&response_type=code"
        ))
        .is_err());
}

#[tokio::test]
async fn session_requires_a_valid_bearer_and_names_the_account() {
    let server = MockServer::start().await;
    let http = reqwest::Client::new();
    let r = http.get(format!("{}/.well-known/jmap", server.url())).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401);
    assert!(connect(&server, "garbage").await.is_err_and(|e| is_status(&e, "401")));

    let client = signed_in(&server).await;
    let session = client.session();
    assert_eq!(session.username(), USERNAME);
    assert!(session.has_capability("urn:ietf:params:jmap:mail"));
    assert!(session.has_capability("urn:ietf:params:jmap:calendars"));
    assert_eq!(client.default_account_id(), ACCOUNT_ID);
    assert_eq!(session.api_url(), format!("{}/jmap", server.url()));
    assert!(session.event_source_url().contains("{types}"));
}

#[tokio::test]
async fn find_inbox_query_unread_and_read_headers() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let inbox = inbox_id(&client).await;
    assert_eq!(inbox, INBOX_ID);

    // Seeded: two unread and one read message in the Inbox, newest unread first.
    let unread = unread_ids(&client, &inbox).await;
    assert_eq!(unread, ["e1", "e2"]);

    let mb = client.mailbox_get(&inbox, None::<Vec<mailbox::Property>>).await.unwrap().unwrap();
    assert_eq!((mb.total_emails(), mb.unread_emails()), (3, 2));
    assert_eq!(mb.role(), mailbox::Role::Inbox);

    let mut req = client.build();
    req.get_email().ids(unread.iter().map(String::as_str)).properties([
        email::Property::Id,
        email::Property::From,
        email::Property::Subject,
        email::Property::Preview,
        email::Property::ReceivedAt,
    ]);
    let list = req.send_single::<EmailGetResponse>().await.unwrap().take_list();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].subject(), Some("Lunch on Friday?"));
    assert_eq!(list[0].from().unwrap()[0].email(), "maria@example.test");
    assert_eq!(list[0].from().unwrap()[0].name(), Some("Maria Keller"));
    assert!(list[0].preview().unwrap().starts_with("Are you free"));
    assert!(list[0].received_at().unwrap() > list[1].received_at().unwrap());

    // Properties not asked for are not sent.
    assert!(list[0].keywords().is_empty());
}

#[tokio::test]
async fn query_sort_position_limit_total_and_back_references() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let mut req = client.build();
    req.query_email()
        .filter(email::query::Filter::in_mailbox(INBOX_ID))
        .sort([Comparator::received_at().ascending()])
        .position(1)
        .limit(1)
        .calculate_total(true);
    let resp = req.send_single::<QueryResponse>().await.unwrap();
    assert_eq!(resp.ids(), ["e2"]); // oldest first: e3, e2, e1
    assert_eq!(resp.total(), Some(3));
    assert_eq!(resp.position(), 1);

    // Email/query then Email/get through "#ids".
    let mut req = client.build();
    let q = req.query_email();
    q.filter(email::query::Filter::in_mailbox(INBOX_ID)).limit(2);
    let reference = req.last_result_reference("/ids");
    req.get_email().ids_ref(reference).properties([email::Property::Subject]);
    let mut response = req.send().await.unwrap();
    let get = response.pop_method_response().unwrap().unwrap_get_email().unwrap();
    assert_eq!(get.list().len(), 2);
    assert_eq!(get.list()[0].subject(), Some("Lunch on Friday?"));
}

#[tokio::test]
async fn unknown_method_is_a_method_error() {
    let server = MockServer::start().await;
    let token = server.issue_access_token();
    let body = json!({
        "using": ["urn:ietf:params:jmap:core"],
        "methodCalls": [["Identity/get", { "accountId": ACCOUNT_ID, "ids": [] }, "c1"]]
    });
    let v = body_json(
        post_json(&reqwest::Client::new(), format!("{}/jmap", server.url()), &body)
            .bearer_auth(token)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(v["methodResponses"][0][0], "error");
    assert_eq!(v["methodResponses"][0][1]["type"], "unknownMethod");
    assert_eq!(v["methodResponses"][0][2], "c1");
}

#[tokio::test]
async fn changes_follow_deliver_mark_read_and_destroy() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let es0 = email_query_state(&client).await;
    let ms0 = mailbox_state(&client).await;

    let new = server.deliver("Zed", "zed@example.test", "Hello", "first");
    let changes = client.email_changes(&es0, None).await.unwrap();
    assert_eq!(changes.created(), [new.as_str()]);
    assert!(changes.updated().is_empty() && changes.destroyed().is_empty() && !changes.has_more_changes());
    assert_eq!(changes.old_state(), es0);
    let es1 = changes.new_state().to_string();
    assert_ne!(es1, es0);

    // Counts moved, so the Inbox is "updated" and its counts are the ones that changed.
    let mchanges = client.mailbox_changes(&ms0, 50).await.unwrap();
    assert_eq!(mchanges.updated(), [INBOX_ID]);
    let ms1 = mchanges.new_state().to_string();
    let mb = client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.unwrap().unwrap();
    assert_eq!((mb.total_emails(), mb.unread_emails()), (4, 3));

    server.mark_read("e1");
    server.destroy("e2");
    let changes = client.email_changes(&es1, None).await.unwrap();
    assert_eq!(changes.updated(), ["e1"]);
    assert_eq!(changes.destroyed(), ["e2"]);
    assert!(changes.created().is_empty());
    let mb = client.mailbox_changes(&ms1, 50).await.unwrap();
    assert_eq!(mb.updated(), [INBOX_ID]);

    // Created then destroyed between two looks is not reported at all.
    let fresh_state = changes.new_state().to_string();
    let tmp = server.deliver("Zed", "zed@example.test", "Temp", "gone soon");
    server.destroy(&tmp);
    let changes = client.email_changes(&fresh_state, None).await.unwrap();
    assert!(changes.created().is_empty() && changes.destroyed().is_empty());

    // Asking from the current state gives nothing.
    let changes = client.email_changes(changes.new_state(), None).await.unwrap();
    assert_eq!(changes.total_changes(), 0);
}

#[tokio::test]
async fn max_changes_pages_through_the_log() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let start = email_query_state(&client).await;
    let ids: Vec<String> = (0..5).map(|i| server.deliver("A", "a@example.test", &format!("m{i}"), "p")).collect();

    let mut state = start;
    let mut seen = Vec::new();
    let mut pages = 0;
    loop {
        let mut page = client.email_changes(&state, Some(2)).await.unwrap();
        assert!(page.created().len() <= 2);
        seen.extend(page.take_created());
        state = page.take_new_state();
        pages += 1;
        if !page.has_more_changes() {
            break;
        }
        assert!(pages < 10);
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, ids);
}

#[tokio::test]
async fn forgotten_states_cannot_calculate_changes() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let old = email_query_state(&client).await;
    server.deliver("A", "a@example.test", "x", "p");
    server.forget_changes_before_now();

    let err = client.email_changes(&old, None).await.unwrap_err();
    assert!(
        matches!(&err, Error::Method(m) if m.p_type == jmap_client::core::error::MethodErrorType::CannotCalculateChanges),
        "{err:?}"
    );
    let err = client.mailbox_changes("ms1", 10).await.unwrap_err();
    assert!(
        matches!(&err, Error::Method(m) if m.p_type == jmap_client::core::error::MethodErrorType::CannotCalculateChanges)
    );

    // The current state is still fine, and so are later ones.
    let now = email_query_state(&client).await;
    assert_eq!(client.email_changes(&now, None).await.unwrap().total_changes(), 0);
    server.deliver("A", "a@example.test", "y", "p");
    assert_eq!(client.email_changes(&now, None).await.unwrap().created().len(), 1);
    // A state the server never issued is just as unknown.
    assert!(client.email_changes("es99999", None).await.is_err());
}

#[tokio::test]
async fn email_set_marks_read_and_moves() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let before = email_query_state(&client).await;

    client.email_set_keyword("e1", "$seen", true).await.unwrap();
    assert!(server.is_read("e1"));
    client.email_set_keyword("e1", "$seen", false).await.unwrap();
    assert!(!server.is_read("e1"));

    // Archive: add one mailbox, remove the other, in one patch.
    let mut req = client.build();
    req.set_email().update("e2").mailbox_id(ARCHIVE_ID, true).mailbox_id(INBOX_ID, false);
    req.send_single::<jmap_client::core::response::EmailSetResponse>().await.unwrap().updated("e2").unwrap();
    assert_eq!(server.mailboxes_of("e2"), [ARCHIVE_ID]);
    assert_eq!(server.roles_of("e2"), ["archive"]);

    // Whole-property replacement.
    client.email_set_mailboxes("e2", [TRASH_ID]).await.unwrap();
    assert_eq!(server.roles_of("e2"), ["trash"]);
    client.email_set_keywords("e2", ["$seen", "$flagged"]).await.unwrap();
    assert_eq!(server.keywords_of("e2"), ["$flagged", "$seen"]);

    let mb = client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.unwrap().unwrap();
    assert_eq!((mb.total_emails(), mb.unread_emails()), (2, 1)); // e1 unread, e3 read
    let trash = client.mailbox_get(TRASH_ID, None::<Vec<mailbox::Property>>).await.unwrap().unwrap();
    assert_eq!((trash.total_emails(), trash.unread_emails()), (1, 0));

    // Everything above shows up in the change log.
    let changes = client.email_changes(&before, None).await.unwrap();
    assert_eq!(changes.updated().len(), 2);

    // Mistakes are SetErrors, not crashes.
    assert!(client.email_set_keyword("nope", "$seen", true).await.is_err());
    assert!(client.email_set_mailboxes("e1", Vec::<String>::new()).await.is_err());
    assert!(client.email_set_mailbox("e1", "mb-does-not-exist", true).await.is_err());

    client.email_destroy("e3").await.unwrap();
    assert!(!server.exists("e3"));
}

#[tokio::test]
async fn event_stream_delivers_state_changes() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let mut stream =
        client.event_source(Some([DataType::Email, DataType::Mailbox]), false, Some(1), None).await.unwrap();
    server.wait_for_event_streams(1).await;

    let _ = server.deliver("Zed", "zed@example.test", "Push me", "hello");
    let first = tokio::time::timeout(T, stream.next()).await.expect("event in time").unwrap().unwrap();
    let PushNotification::StateChange(changes) = first else { panic!("not a state change") };
    assert!(changes.has_type(DataType::Email) && changes.has_type(DataType::Mailbox));
    let email_state = changes.changes(ACCOUNT_ID).unwrap().find(|(t, _)| **t == DataType::Email).unwrap().1.clone();
    assert_eq!(email_state, server.email_state());

    // Pings every second must neither end the stream nor surface as errors.
    assert!(tokio::time::timeout(Duration::from_millis(2300), stream.next()).await.is_err());
    assert_eq!(server.open_event_streams(), 1);

    // A change made by an Email/set call is pushed too.
    client.email_set_keyword("e1", "$seen", true).await.unwrap();
    let second = tokio::time::timeout(T, stream.next()).await.expect("event in time").unwrap().unwrap();
    assert!(matches!(second, PushNotification::StateChange(_)));

    // A no-op change is not an event.
    server.mark_read("e1");
    assert!(tokio::time::timeout(Duration::from_millis(300), stream.next()).await.is_err());

    // Silently dead: nothing is pushed while disabled, and the change is still readable.
    server.set_push_enabled(false);
    let before = email_query_state(&client).await;
    server.deliver("Zed", "zed@example.test", "Quiet", "no event");
    assert!(tokio::time::timeout(Duration::from_millis(300), stream.next()).await.is_err());
    assert_eq!(client.email_changes(&before, None).await.unwrap().created().len(), 1);
}

#[tokio::test]
async fn event_stream_honours_types_and_closeafter() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;

    // Only Thread: a delivery changes Email and Mailbox, so nothing arrives.
    let mut threads = client.event_source(Some([DataType::Thread]), false, None, None).await.unwrap();
    // closeafter=state: one event, then the server ends the stream.
    let mut once = client.event_source(Some([DataType::Email]), true, None, None).await.unwrap();
    server.wait_for_event_streams(2).await;

    server.deliver("Zed", "zed@example.test", "x", "p");
    assert!(matches!(tokio::time::timeout(T, once.next()).await.unwrap(), Some(Ok(PushNotification::StateChange(_)))));
    assert!(tokio::time::timeout(T, once.next()).await.unwrap().is_none());
    assert!(tokio::time::timeout(Duration::from_millis(300), threads.next()).await.is_err());
    server.wait_for_event_streams(1).await;
}

#[tokio::test]
async fn drop_event_streams_ends_the_stream() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let mut a = client.event_source(None::<Vec<DataType>>, false, None, None).await.unwrap();
    let mut b = client.event_source(None::<Vec<DataType>>, false, None, None).await.unwrap();
    server.wait_for_event_streams(2).await;

    server.drop_event_streams();
    assert!(tokio::time::timeout(T, a.next()).await.unwrap().is_none());
    assert!(tokio::time::timeout(T, b.next()).await.unwrap().is_none());
    server.wait_for_event_streams(0).await;

    // A stream opened afterwards works again.
    let mut c = client.event_source(None::<Vec<DataType>>, false, None, None).await.unwrap();
    server.wait_for_event_streams(1).await;
    server.deliver("Zed", "zed@example.test", "again", "p");
    assert!(matches!(tokio::time::timeout(T, c.next()).await.unwrap(), Some(Ok(PushNotification::StateChange(_)))));
}

#[tokio::test]
async fn expired_tokens_get_401_everywhere() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    assert!(client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.is_ok());

    server.expire_access_tokens();
    let err = client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.unwrap_err();
    assert!(is_status(&err, "401"), "{err:?}");
    assert!(client
        .event_source(None::<Vec<DataType>>, false, None, None)
        .await
        .err()
        .is_some_and(|e| is_status(&e, "401")));
    assert!(client.refresh_session().await.is_err());

    // A token issued after the expiry is fine.
    assert!(signed_in(&server).await.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.is_ok());
}

#[tokio::test]
async fn short_token_lifetime_runs_out() {
    let server = MockServer::start().await;
    server.set_access_token_lifetime(1);
    let token = server.issue_access_token();
    let client = connect(&server, &token).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let err = client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.unwrap_err();
    assert!(is_status(&err, "401"), "{err:?}");
}

#[tokio::test]
async fn set_down_gives_503_and_closes_streams() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let mut stream = client.event_source(None::<Vec<DataType>>, false, None, None).await.unwrap();
    server.wait_for_event_streams(1).await;

    server.set_down(true);
    assert!(tokio::time::timeout(T, stream.next()).await.unwrap().is_none());
    let err = client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.unwrap_err();
    assert!(is_status(&err, "503"), "{err:?}");
    assert!(connect(&server, &server.issue_access_token()).await.is_err());
    let (status, _) =
        post_form(&format!("{}/token", server.url()), &[("grant_type", "refresh_token"), ("refresh_token", "x")]).await;
    assert_eq!(status, 503);
    assert!(server.request_count("/jmap") >= 1 && server.request_count("/.well-known/jmap") >= 2);

    server.set_down(false);
    assert!(client.mailbox_get(INBOX_ID, None::<Vec<mailbox::Property>>).await.is_ok());
}

#[tokio::test]
async fn request_counts_by_path_and_method() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    assert_eq!(server.request_count("/.well-known/jmap"), 1);
    let inbox = inbox_id(&client).await;
    unread_ids(&client, &inbox).await;
    assert_eq!(server.request_count("Mailbox/query"), 1);
    assert_eq!(server.request_count("Email/query"), 1);
    assert_eq!(server.request_count("/jmap"), 2);
    server.reset_request_counts();
    assert_eq!(server.request_count("/jmap"), 0);
}

#[tokio::test]
async fn seeded_data_is_as_documented() {
    let server = MockServer::start().await;
    assert_eq!(server.all_message_ids(), ["e1", "e2", "e3", "e4", "e5"]);
    assert_eq!(server.roles_of("e1"), ["inbox"]);
    assert_eq!(server.roles_of("e4"), ["archive"]);
    assert_eq!(server.roles_of("e5"), ["sent"]);
    assert!(!server.is_read("e1") && !server.is_read("e2") && server.is_read("e3"));
    let client = signed_in(&server).await;
    let mut req = client.build();
    req.get_mailbox();
    let list = req.send_single::<MailboxGetResponse>().await.unwrap().take_list();
    let roles: Vec<_> = list.iter().map(|m| (m.id().unwrap().to_string(), m.name().unwrap().to_string())).collect();
    assert_eq!(
        roles,
        [("mb1", "Inbox"), ("mb2", "Archive"), ("mb3", "Trash"), ("mb4", "Drafts"), ("mb5", "Sent"), ("mb6", "Junk")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
}

// ---- calendar ---------------------------------------------------------------------------

/// `CalendarEvent/get` as the core sends it: one plain JMAP request.
async fn calendar_get(server: &MockServer, ids: Value, properties: Value) -> Value {
    let body = json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:calendars"],
        "methodCalls": [["CalendarEvent/get", { "accountId": ACCOUNT_ID, "ids": ids, "properties": properties }, "a"]]
    });
    let response = reqwest::Client::new()
        .post(format!("{}/jmap", server.url()))
        .bearer_auth(server.issue_access_token())
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let answer: Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(answer["methodResponses"][0][0], "CalendarEvent/get", "{answer}");
    answer["methodResponses"][0][1].clone()
}

#[tokio::test]
async fn calendar_events_are_stored_and_fetched_by_id() {
    let server = MockServer::start().await;
    let first = server.add_event(json!({"title": "Standup", "start": "2026-11-11T09:30:00", "description": "long"}));
    let second = server.add_event(json!({"@type": "Task", "title": "Tax", "due": "2026-11-20T17:00:00"}));
    assert_ne!(first, second);

    let got = calendar_get(&server, json!([first, second, "nope"]), json!(["title", "start", "@type"])).await;
    let list = got["list"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    // `id` always comes; properties not asked for do not.
    assert_eq!(list[0], json!({"id": first, "title": "Standup", "start": "2026-11-11T09:30:00", "@type": "Event"}));
    assert_eq!(list[1]["@type"], "Task");
    assert_eq!(got["notFound"], json!(["nope"]));

    server.remove_event(&first);
    let got = calendar_get(&server, json!([first]), Value::Null).await;
    assert_eq!((got["list"].as_array().unwrap().len(), got["notFound"].clone()), (0, json!([first])));
    assert_eq!(server.request_count("CalendarEvent/get"), 2);
}

#[tokio::test]
async fn fired_alerts_reach_streams_that_ask_for_them() {
    let server = MockServer::start().await;
    let client = signed_in(&server).await;
    let mut wants = client.event_source(Some([DataType::CalendarAlert]), false, None, None).await.unwrap();
    let mut mail_only = client.event_source(Some([DataType::Email]), false, None, None).await.unwrap();
    server.wait_for_event_streams(2).await;

    let id = server.add_event(json!({"title": "Call", "start": "2026-11-11T09:30:00"}));
    server.fire_alert(&id, "al1", Some("2026-11-11T09:30:00"));
    server.fire_alert("gone", "al2", None);

    let first = tokio::time::timeout(T, wants.next()).await.expect("alert in time").unwrap().unwrap();
    let PushNotification::CalendarAlert(alert) = first else { panic!("not an alert") };
    assert_eq!(alert.account_id, ACCOUNT_ID);
    assert_eq!((alert.calendar_event_id.as_str(), alert.alert_id.as_str()), (id.as_str(), "al1"));
    assert_eq!(alert.recurrence_id.as_deref(), Some("2026-11-11T09:30:00"));
    assert_eq!(alert.uid, format!("uid-{id}"));

    // An alert for an event that no longer exists is still pushed, without a recurrence id.
    let second = tokio::time::timeout(T, wants.next()).await.expect("alert in time").unwrap().unwrap();
    let PushNotification::CalendarAlert(alert) = second else { panic!("not an alert") };
    assert_eq!((alert.calendar_event_id.as_str(), alert.recurrence_id), ("gone", None));

    // The stream that did not ask for alerts heard nothing.
    assert!(tokio::time::timeout(Duration::from_millis(200), mail_only.next()).await.is_err());
}
