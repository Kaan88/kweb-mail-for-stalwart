//! M12 stage 6, the sender check, through the `Core` object against the mock server. The header
//! forms are Stalwart 1.1.0's own (probed, `dev/notes/stalwart-sender-check.md`); the parsing and
//! the rules for which header is believed are unit tests in `sender_check.rs`.

use std::sync::Arc;

use crate::support::MockServer;
use kweb_core::{
    ffi::{Core, Shade},
    model::{MethodResult, SenderReason, SenderStatus},
    secrets::MemorySecrets,
};
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    _rt: Runtime,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        core.check_once(Shade::Unknown).expect("a pass");
        Rig { core, server, _rt: rt, _dir: dir }
    }
}

const RECEIVED: &str = " from mail.example.org (relay [127.0.0.1])\r\n\tby localhost (Stalwart SMTP) with ESMTP id 1;\r\n\tMon, 16 Nov 2026 14:26:01 +0000";

#[test]
fn the_servers_own_results_give_the_status_and_a_forged_copy_is_ignored() {
    let rig = Rig::new();
    // Stalwart's own header: DMARC pass for the From domain (the entry names no domain here).
    rig.server.set_raw_headers(
        "e1",
        &[
            ("Received", RECEIVED),
            (
                "Authentication-Results",
                " localhost;\r\n\tdkim=pass header.d=example.org;\r\n\tdmarc=pass header.from=example.org",
            ),
            ("From", " x"),
        ],
    );
    let check = rig.core.sender_check("e1".into()).unwrap();
    assert_eq!(
        check.dmarc,
        MethodResult::NotRun,
        "the mock's sender is not example.org, so this result is about another domain"
    );
    assert_ne!(check.status, SenderStatus::Failed);

    // A header written by the sender, with the right name, below the sender's From: not believed.
    rig.server.set_raw_headers(
        "e1",
        &[("Received", RECEIVED), ("From", " x"), ("Authentication-Results", " localhost; dmarc=pass")],
    );
    let check = rig.core.sender_check("e1".into()).unwrap();
    assert_eq!((check.status, check.reason), (SenderStatus::NotChecked, SenderReason::NotFromThisServer));
    assert_eq!(check.dmarc, MethodResult::NotRun);
}

#[test]
fn a_dmarc_fail_is_failed_and_a_pass_is_verified() {
    let rig = Rig::new();
    let set = |result: &str| {
        let value = format!(" localhost;\r\n\tspf=pass smtp.mailfrom=bounce@other.example;\r\n\tdmarc={result}");
        rig.server.set_raw_headers("e1", &[("Received", RECEIVED), ("Authentication-Results", &value), ("From", " x")]);
        rig.core.sender_check("e1".into()).unwrap()
    };
    let failed = set("fail (policy not aligned) policy.dmarc=reject");
    assert_eq!((failed.status, failed.reason), (SenderStatus::Failed, SenderReason::DmarcFail));
    assert_eq!(failed.spf, MethodResult::Other, "an SPF pass for another domain does not count");
    let verified = set("pass");
    assert_eq!((verified.status, verified.reason), (SenderStatus::Verified, SenderReason::DmarcPass));
}

#[test]
fn a_message_with_no_header_block_or_gone_is_told_plainly() {
    let rig = Rig::new();
    // Nothing set: no headers at all, so nothing the server wrote.
    let check = rig.core.sender_check("e1".into()).unwrap();
    assert_eq!(check.status, SenderStatus::NotChecked);
    assert!(rig.core.sender_check("no-such".into()).is_err());
    assert!(rig.server.request_count("Email/get") > 0);
}
