//! M9 stage 0: the structure of a message and its raw bytes over JMAP, and the exact byte range
//! of the part a signature covers.

use crate::support::*;
use kweb_core::{
    crypto::mime,
    jmap::{Connection, MAX_RAW_BYTES},
    Error,
};

fn fx(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/crypto/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

async fn connect(server: &MockServer) -> Connection {
    Connection::connect(&server.url(), &server.issue_access_token()).await.expect("connect")
}

#[tokio::test]
async fn the_structure_names_the_types_of_the_parts_without_the_message() {
    let server = MockServer::start().await;
    let conn = connect(&server).await;
    let signed = server.deliver("Alice", "alice@example.test", "Signed", "x");
    server.set_raw(&signed, &fx("pgp-signed.eml"));
    let plain = server.deliver("Bob", "bob@example.test", "Plain", "x");
    let found = conn.message_structure(&signed).await.unwrap().unwrap();
    assert_eq!(found.tree.media, "multipart/signed");
    let kids: Vec<_> = found.tree.children.iter().map(|c| c.media.as_str()).collect();
    assert_eq!(kids, ["text/plain", "application/pgp-signature"]);
    assert_eq!(found.size, fx("pgp-signed.eml").len() as u64);
    assert!(!found.blob_id.is_empty());
    let other = conn.message_structure(&plain).await.unwrap().unwrap();
    assert_eq!((other.tree.media.as_str(), other.tree.children.len()), ("text/plain", 0));
    assert!(conn.message_structure("nope").await.unwrap().is_none());
}

#[tokio::test]
async fn the_raw_bytes_come_whole_and_the_signed_range_is_exact() {
    let server = MockServer::start().await;
    let conn = connect(&server).await;
    let id = server.deliver("Alice", "alice@example.test", "Signed", "x");
    let sent = fx("pgp-signed.eml");
    server.set_raw(&id, &sent);
    let structure = conn.message_structure(&id).await.unwrap().unwrap();
    let raw = conn.raw_message(&structure.blob_id, MAX_RAW_BYTES).await.unwrap();
    assert_eq!(raw, sent);
    let root = mime::parse(&raw).unwrap();
    let first = &raw[root.children[0].whole.clone()];
    assert!(first.starts_with(b"Content-Type: text/plain"));
    assert!(first.ends_with(b"Hello Bob, this is signed."), "ends before the line ending of the delimiter");
}

#[tokio::test]
async fn a_message_over_the_limit_is_refused_not_cut() {
    let server = MockServer::start().await;
    let conn = connect(&server).await;
    let id = server.deliver("Alice", "alice@example.test", "Big", "x");
    server.set_raw(&id, &vec![b'x'; 5000]);
    let structure = conn.message_structure(&id).await.unwrap().unwrap();
    let refused = conn.raw_message(&structure.blob_id, 1000).await.unwrap_err();
    assert!(matches!(refused, Error::Invalid { ref msg } if msg.contains("too large")), "{refused:?}");
    assert_eq!(conn.raw_message(&structure.blob_id, 5000).await.unwrap().len(), 5000);
}
