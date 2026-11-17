//! Public links to files against the mock of the fork's share-link API
//! (`support/public_links.rs`), through the `Core` object the way the app uses it.

use std::sync::Arc;

use crate::support::{MockServer, ACCOUNT_ID, SHARED_ACCOUNT};
use kweb_core::{
    ffi::Core,
    model::{FileEntry, FileKind},
    secrets::MemorySecrets,
    Error,
};
use tokio::runtime::Runtime;

struct Rig {
    core: Arc<Core>,
    server: MockServer,
    _dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    fn with(links: bool) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server.enable_files();
        if links {
            server.enable_public_links();
        }
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, _dir: dir, _rt: rt }
    }

    /// A file of the user's own space, as the listing gives it.
    fn own_file(&self, name: &str, bytes: &[u8]) -> FileEntry {
        self.server.add_file(ACCOUNT_ID, None, name, bytes, "application/pdf");
        let listing = self.core.files(None, None).unwrap();
        listing.entries.into_iter().find(|e| e.name == name).unwrap()
    }
}

#[test]
fn official_server_has_no_public_links() {
    let rig = Rig::with(false);
    assert_eq!(rig.core.public_link_limits().unwrap(), None);
}

#[test]
fn limits_make_list_and_revoke() {
    let rig = Rig::with(true);
    let limits = rig.core.public_link_limits().unwrap().unwrap();
    assert_eq!((limits.min_expires_in, limits.max_expires_in, limits.max_per_file), (3600, 365 * 86_400, 20));

    let file = rig.own_file("report.pdf", b"twelve bytes");
    assert!(rig.core.public_links(file.clone()).unwrap().is_empty());

    let first = rig.core.public_link_create(file.clone(), 86_400).unwrap();
    let second = rig.core.public_link_create(file.clone(), 10).unwrap();
    // The link is the server's own address and the path it returned, never another host.
    assert!(first.url.starts_with(&format!("{}/s/", rig.server.url())), "{}", first.url);
    assert_eq!(first.expires - first.created, 86_400 - 1);
    assert!(second.expires - second.created >= 3600 - 2, "kept within the server's minimum");

    let links = rig.core.public_links(file.clone()).unwrap();
    assert_eq!(links.iter().map(|l| l.id.clone()).collect::<Vec<_>>(), vec![second.id.clone(), first.id.clone()]);

    rig.core.public_link_revoke(file.clone(), first.id.clone()).unwrap();
    assert_eq!(rig.server.public_link_count(&file.id), 1);
    // Revoking again is not an error.
    rig.core.public_link_revoke(file.clone(), first.id).unwrap();
    assert_eq!(rig.core.public_links(file).unwrap(), vec![second]);
}

#[test]
fn every_request_names_the_file_so_the_server_can_check_it() {
    let rig = Rig::with(true);
    let file = rig.own_file("report.pdf", b"twelve bytes");
    rig.core.public_link_create(file.clone(), 3600).unwrap();
    let requests = rig.server.public_link_requests();
    let (_, body) = requests.iter().find(|(action, _)| action == "create").unwrap();
    assert_eq!(body["fileId"], file.id.as_str());
    assert_eq!(body["expectName"], "report.pdf");
    assert_eq!(body["expectSize"], 12);

    // A file that changed since it was listed is refused with the server's words.
    let mut stale = file;
    stale.size = 99;
    match rig.core.public_link_create(stale, 3600) {
        Err(Error::Invalid { msg }) => assert!(msg.contains("does not match"), "{msg}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn only_own_files_never_folders_or_shared_spaces() {
    let rig = Rig::with(true);
    let mut file = rig.own_file("report.pdf", b"x");
    let mut folder = file.clone();
    folder.kind = FileKind::Folder;
    assert!(matches!(rig.core.public_link_create(folder, 3600), Err(Error::Invalid { .. })));
    file.account_id = Some(SHARED_ACCOUNT.to_string());
    assert!(matches!(rig.core.public_links(file), Err(Error::Invalid { .. })));
    assert!(rig.server.public_link_requests().iter().all(|(action, _)| action == "limits"), "nothing was sent");
}

#[test]
fn too_many_links_is_the_servers_refusal() {
    let rig = Rig::with(true);
    let file = rig.own_file("report.pdf", b"x");
    for _ in 0..20 {
        rig.core.public_link_create(file.clone(), 3600).unwrap();
    }
    match rig.core.public_link_create(file, 3600) {
        Err(Error::Invalid { msg }) => assert!(msg.contains("Too many links"), "{msg}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}
