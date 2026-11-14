//! M7 file storage against the mock server, through the `Core` object the way the app uses it:
//! the spaces, listings, search, folders, upload and download, in the user's own space and in a
//! space another account shares. The mock behaves like Stalwart 1.1.0 where it matters (see
//! `support/files.rs`).

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use kweb_core::{
    ffi::Core,
    model::{FileEntry, FileKind},
    secrets::MemorySecrets,
    Error,
};
use support::{MockServer, ACCOUNT_ID, EMPTY_SHARE_ACCOUNT, MAIL_ONLY_ACCOUNT, SHARED_ACCOUNT};
use tokio::runtime::Runtime;

struct Rig {
    // Field order is drop order: the core goes first, the mock and its runtime last.
    core: Arc<Core>,
    server: MockServer,
    dir: tempfile::TempDir,
    _rt: Runtime,
}

impl Rig {
    /// A mock server with file storage, set up by `setup` before the core signs in.
    fn with(setup: impl FnOnce(&MockServer)) -> Rig {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let server = rt.block_on(MockServer::start());
        server.enable_files();
        setup(&server);
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cache.db").to_string_lossy().into_owned();
        let core = Core::new(db_path, Arc::new(MemorySecrets::default())).unwrap();
        let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
        core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
        Rig { core, server, dir, _rt: rt }
    }

    fn new() -> Rig {
        Rig::with(|_| {})
    }

    /// A file in the rig's directory with this content.
    fn local(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn names(&self, account: Option<&str>, folder: Option<&str>) -> Vec<String> {
        let listing = self.core.files(account.map(str::to_string), folder.map(str::to_string)).unwrap();
        listing.entries.into_iter().map(|e| e.name).collect()
    }

    fn upload(
        &self,
        account: Option<&str>,
        parent: Option<&str>,
        name: &str,
        path: &Path,
        replace: bool,
    ) -> Result<FileEntry, Error> {
        self.core.file_upload(
            account.map(str::to_string),
            parent.map(str::to_string),
            name.into(),
            "text/plain".into(),
            path.to_string_lossy().into_owned(),
            replace,
        )
    }
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

fn invalid(e: Error) -> String {
    match e {
        Error::Invalid { msg } => msg,
        other => panic!("expected Error::Invalid, got {other:?}"),
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

// ---- spaces ----

#[test]
fn a_server_without_file_storage_has_no_spaces() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let server = rt.block_on(MockServer::start());
    let dir = tempfile::tempdir().unwrap();
    let core =
        Core::new(dir.path().join("c.db").to_string_lossy().into_owned(), Arc::new(MemorySecrets::default())).unwrap();
    let url = core.begin_sign_in_with_redirect(server.url(), "ovh.kweb.mail:/oauth/callback".into()).unwrap();
    core.complete_sign_in(server.authorize(&url).unwrap()).unwrap();
    assert!(core.file_spaces().unwrap().is_empty());
    assert!(invalid(core.files(None, None).unwrap_err()).contains("no file storage"));
}

#[test]
fn the_spaces_are_the_own_one_and_those_that_share_files() {
    let rig = Rig::new();
    let spaces = rig.core.file_spaces().unwrap();
    // The account that has the capability but shares nothing, and the one without it, are not there.
    assert_eq!(spaces.len(), 2, "{spaces:?}");
    assert_eq!((spaces[0].account_id.clone(), spaces[0].name.as_str()), (None, ""));
    assert!(spaces[0].may_add_at_top);
    assert_eq!(spaces[0].max_upload, 50_000_000);
    assert_eq!((spaces[1].account_id.clone(), spaces[1].name.as_str()), (s(SHARED_ACCOUNT), "shared@kweb.test"));
    assert!(!spaces[1].may_add_at_top);
    // The probe asked both accounts that have the capability; the empty one said no.
    assert_eq!(rig.server.request_count("FileNode/query"), 2);
}

#[test]
fn the_own_top_level_may_be_closed() {
    let rig = Rig::with(|server| server.set_files_top_level_allowed(false));
    assert!(!rig.core.file_spaces().unwrap()[0].may_add_at_top);
    assert!(!rig.core.files(None, None).unwrap().may_add);
    let words = invalid(rig.core.file_create_folder(None, None, "Top".into()).unwrap_err());
    assert!(words.contains("not allowed"), "{words}");
}

#[test]
fn a_space_that_is_not_in_the_session_is_invalid() {
    let rig = Rig::new();
    for account in ["nobody", MAIL_ONLY_ACCOUNT] {
        assert!(invalid(rig.core.files(s(account), None).unwrap_err()).contains("no such file space"));
        assert!(rig.core.file_delete(s(account), "1".into()).is_err());
    }
    // Nothing was sent for a space that is not there.
    assert_eq!(rig.server.request_count("FileNode/set"), 0);
    let _ = EMPTY_SHARE_ACCOUNT;
}

// ---- listing ----

#[test]
fn a_listing_has_folders_first_then_files_by_name_without_regard_to_case() {
    let rig = Rig::new();
    let docs = rig.server.add_folder(ACCOUNT_ID, None, "docs", None);
    rig.server.add_folder(ACCOUNT_ID, None, "Archive", None);
    rig.server.add_file(ACCOUNT_ID, None, "b.txt", b"12345", "text/plain");
    rig.server.add_file(ACCOUNT_ID, None, "A.txt", b"", "text/plain");
    rig.server.add_file(ACCOUNT_ID, Some(&docs), "inner.txt", b"x", "text/plain");
    let top = rig.core.files(None, None).unwrap();
    assert!(top.folder.is_none() && top.path.is_empty() && top.may_add);
    assert_eq!(top.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Archive", "docs", "A.txt", "b.txt"]);
    let b = &top.entries[3];
    assert_eq!((b.kind, b.size, b.media_type.as_str(), b.account_id.clone()), (FileKind::File, 5, "text/plain", None));
    assert!(b.blob_id.is_some() && b.modified == Some(1_577_836_800) && b.parent_id.is_none());
    // Files at the top level of the own space have every right; a folder has its own.
    assert!(b.may_rename && b.may_delete && b.may_replace && !b.may_add);
    let folder = &top.entries[1];
    assert!(folder.kind == FileKind::Folder && folder.may_add && folder.blob_id.is_none() && folder.size == 0);
    assert!(folder.media_type.is_empty() && !folder.may_replace);
}

#[test]
fn a_folder_shows_its_path_down_to_itself() {
    let rig = Rig::new();
    let a = rig.server.add_folder(ACCOUNT_ID, None, "a", None);
    let b = rig.server.add_folder(ACCOUNT_ID, Some(&a), "b", None);
    let c = rig.server.add_folder(ACCOUNT_ID, Some(&b), "c", None);
    rig.server.add_file(ACCOUNT_ID, Some(&c), "deep.txt", b"deep", "text/plain");
    let listing = rig.core.files(None, s(&c)).unwrap();
    assert_eq!(listing.path.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(listing.folder.as_ref().map(|f| f.name.as_str()), Some("c"));
    assert!(listing.may_add);
    let file = &listing.entries[0];
    assert_eq!((file.name.as_str(), file.parent_id.clone()), ("deep.txt", s(&c)));
    // A top-level folder is its own whole path.
    assert_eq!(rig.core.files(None, s(&a)).unwrap().path.len(), 1);
}

#[test]
fn a_folder_that_is_gone_or_is_a_file_is_invalid() {
    let rig = Rig::new();
    let file = rig.server.add_file(ACCOUNT_ID, None, "f.txt", b"x", "text/plain");
    assert!(invalid(rig.core.files(None, s("999")).unwrap_err()).contains("no longer there"));
    assert!(invalid(rig.core.files(None, s(&file)).unwrap_err()).contains("no longer there"));
}

#[test]
fn a_listing_follows_the_servers_limits_over_as_many_requests_as_it_needs() {
    // The server takes 3 ids in a get and gives 2 in a query page.
    let rig = Rig::with(|server| server.set_file_limits(3, 2));
    for i in 0..7 {
        rig.server.add_file(ACCOUNT_ID, None, &format!("f{i}.txt"), b"x", "text/plain");
    }
    assert_eq!(rig.names(None, None), ["f0.txt", "f1.txt", "f2.txt", "f3.txt", "f4.txt", "f5.txt", "f6.txt"]);
}

#[test]
fn a_shared_top_level_shows_the_shared_folders_only() {
    let rig = Rig::new();
    rig.server.add_file(SHARED_ACCOUNT, None, "stray.txt", b"x", "text/plain");
    let top = rig.core.files(s(SHARED_ACCOUNT), None).unwrap();
    assert_eq!(top.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Shared"]);
    assert!(!top.may_add);
    let shared = &top.entries[0];
    assert_eq!(shared.account_id, s(SHARED_ACCOUNT));
    assert!(shared.may_add && !shared.may_rename && !shared.may_delete);
}

#[test]
fn rights_in_a_shared_folder_come_from_the_folder() {
    let rig = Rig::new();
    let inside = rig.core.files(s(SHARED_ACCOUNT), s("1")).unwrap();
    assert!(inside.may_add);
    assert_eq!(inside.path.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Shared"]);
    let (read_only, hello) = (&inside.entries[0], &inside.entries[1]);
    assert_eq!((read_only.name.as_str(), hello.name.as_str()), ("ReadOnly", "hello.txt"));
    assert!(!read_only.may_add && !read_only.may_rename && !read_only.may_delete);
    // The folder lets the user change content but not rename or delete.
    assert!(hello.may_replace && !hello.may_rename && !hello.may_delete && !hello.may_add);
    rig.server.add_file(SHARED_ACCOUNT, s("2").as_deref(), "locked.txt", b"x", "text/plain");
    let locked = rig.core.files(s(SHARED_ACCOUNT), s("2")).unwrap();
    assert!(!locked.may_add && !locked.entries[0].may_replace);
}

#[test]
fn the_same_id_in_two_spaces_is_not_mixed_up() {
    let rig = Rig::new();
    // The own space's first node gets id 1, as the shared space's folder does.
    let own = rig.server.add_folder(ACCOUNT_ID, None, "Own", None);
    let own_file = rig.server.add_file(ACCOUNT_ID, s(&own).as_deref(), "mine.txt", b"mine", "text/plain");
    assert_eq!((own.as_str(), own_file.as_str()), ("1", "2"));
    assert_eq!(rig.names(None, s("1").as_deref()), ["mine.txt"]);
    assert_eq!(rig.names(s(SHARED_ACCOUNT).as_deref(), s("1").as_deref()), ["ReadOnly", "hello.txt"]);
    // A change names its space: id 2 is a file of the own space and a folder of the shared one.
    rig.core.file_rename(None, "2".into(), "renamed.txt".into()).unwrap();
    assert_eq!(rig.names(None, s("1").as_deref()), ["renamed.txt"]);
    assert_eq!(rig.names(s(SHARED_ACCOUNT).as_deref(), s("1").as_deref()), ["ReadOnly", "hello.txt"]);
    rig.core.file_delete(None, "1".into()).unwrap();
    assert_eq!(rig.server.file_nodes(ACCOUNT_ID).len(), 0);
    assert_eq!(rig.server.file_nodes(SHARED_ACCOUNT).len(), 3);
}

// ---- search ----

#[test]
fn a_search_finds_names_and_content_and_a_blank_one_asks_nothing() {
    let rig = Rig::new();
    let docs = rig.server.add_folder(ACCOUNT_ID, None, "Reports", None);
    rig.server.add_file(ACCOUNT_ID, s(&docs).as_deref(), "q3.txt", b"budget for the year", "text/plain");
    rig.server.add_file(ACCOUNT_ID, None, "budget.md", b"x", "text/markdown");
    rig.server.add_file(ACCOUNT_ID, None, "other.txt", b"x", "text/plain");
    let found = rig.core.file_search(None, "budget".into()).unwrap();
    let mut names: Vec<&str> = found.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["budget.md", "q3.txt"]);
    let q3 = found.iter().find(|e| e.name == "q3.txt").unwrap();
    assert_eq!(q3.parent_id, s(&docs));
    assert!(q3.may_replace && q3.may_delete);
    let before = rig.server.request_count("FileNode/query");
    assert!(rig.core.file_search(None, "   ".into()).unwrap().is_empty());
    assert_eq!(rig.server.request_count("FileNode/query"), before);
    // In a shared space a file takes its rights from its folder.
    let hello = rig.core.file_search(s(SHARED_ACCOUNT), "hello".into()).unwrap();
    assert_eq!(hello.len(), 1);
    assert!(hello[0].may_replace && !hello[0].may_delete);
}

#[test]
fn a_search_gives_at_most_200() {
    let rig = Rig::new();
    for i in 0..205 {
        rig.server.add_file(ACCOUNT_ID, None, &format!("note{i}.txt"), b"x", "text/plain");
    }
    assert_eq!(rig.core.file_search(None, "note".into()).unwrap().len(), 200);
}

#[test]
fn a_search_pages_by_what_the_server_takes_in_a_get() {
    let rig = Rig::with(|server| server.set_file_limits(3, 100));
    for i in 0..8 {
        rig.server.add_file(ACCOUNT_ID, None, &format!("note{i}.txt"), b"x", "text/plain");
    }
    assert_eq!(rig.core.file_search(None, "note".into()).unwrap().len(), 8);
}

// ---- folders, rename, move, delete ----

#[test]
fn a_folder_is_made_and_comes_back_as_the_server_has_it() {
    let rig = Rig::new();
    let made = rig.core.file_create_folder(None, None, "Photos".into()).unwrap();
    assert_eq!((made.name.as_str(), made.kind, made.parent_id.clone()), ("Photos", FileKind::Folder, None));
    assert!(made.may_add && made.may_delete);
    let inner = rig.core.file_create_folder(None, s(&made.id), "2026".into()).unwrap();
    assert_eq!(inner.parent_id, s(&made.id));
    assert_eq!(rig.names(None, s(&made.id).as_deref()), ["2026"]);
}

#[test]
fn the_names_the_server_refuses_are_told_in_words() {
    let rig = Rig::new();
    rig.core.file_create_folder(None, None, "Taken".into()).unwrap();
    let words = |name: &str| invalid(rig.core.file_create_folder(None, None, name.into()).unwrap_err());
    assert!(words("Taken").contains("already there"));
    assert!(words("a/b").contains("character"));
    assert!(words("CON").contains("does not allow"));
    assert!(words(&"x".repeat(256)).contains("255"));
    assert!(words("").contains("255"));
    // A parent that is not there.
    let e = rig.core.file_create_folder(None, s("999"), "x".into()).unwrap_err();
    assert!(invalid(e).contains("gone"));
    // Nothing odd got through: the server's own text is not passed on.
    for name in ["a/b", "CON"] {
        assert!(!words(name).contains("Name "));
    }
}

#[test]
fn rename_and_move_say_what_is_wrong() {
    let rig = Rig::new();
    let a = rig.server.add_folder(ACCOUNT_ID, None, "a", None);
    let b = rig.server.add_folder(ACCOUNT_ID, Some(&a), "b", None);
    let f = rig.server.add_file(ACCOUNT_ID, Some(&b), "f.txt", b"x", "text/plain");
    rig.server.add_file(ACCOUNT_ID, None, "f.txt", b"y", "text/plain");
    rig.core.file_rename(None, f.clone(), "g.txt".into()).unwrap();
    assert_eq!(rig.names(None, s(&b).as_deref()), ["g.txt"]);
    // A name the server does not take.
    assert!(invalid(rig.core.file_rename(None, f.clone(), "b/c".into()).unwrap_err()).contains("character"));
    rig.core.file_rename(None, f.clone(), "f.txt".into()).unwrap();
    assert!(invalid(rig.core.file_move(None, f.clone(), None).unwrap_err()).contains("already there"));
    // A folder into itself and into what is inside it.
    assert!(invalid(rig.core.file_move(None, a.clone(), s(&a)).unwrap_err()).contains("into itself"));
    assert!(invalid(rig.core.file_move(None, a.clone(), s(&b)).unwrap_err()).contains("into itself"));
    // Into a folder that is not one.
    assert!(invalid(rig.core.file_move(None, b.clone(), s(&f)).unwrap_err()).contains("not a folder"));
    // A good move, to the top level and into a folder.
    rig.core.file_rename(None, f.clone(), "moved.txt".into()).unwrap();
    rig.core.file_move(None, f.clone(), None).unwrap();
    assert!(rig.names(None, None).contains(&"moved.txt".to_string()));
    rig.core.file_move(None, f.clone(), s(&a)).unwrap();
    assert_eq!(rig.names(None, s(&a).as_deref()), ["b", "moved.txt"]);
    // Something that is gone.
    assert!(invalid(rig.core.file_rename(None, "999".into(), "x".into()).unwrap_err()).contains("no longer there"));
}

#[test]
fn delete_takes_a_folder_with_everything_in_it_and_gone_is_fine() {
    let rig = Rig::new();
    let a = rig.server.add_folder(ACCOUNT_ID, None, "a", None);
    let b = rig.server.add_folder(ACCOUNT_ID, Some(&a), "b", None);
    rig.server.add_file(ACCOUNT_ID, Some(&b), "f.txt", b"x", "text/plain");
    rig.server.add_file(ACCOUNT_ID, None, "keep.txt", b"x", "text/plain");
    rig.core.file_delete(None, a.clone()).unwrap();
    assert_eq!(rig.server.file_nodes(ACCOUNT_ID).len(), 1);
    // Again: it is gone, which is what was wanted.
    rig.core.file_delete(None, a).unwrap();
    rig.core.file_delete(None, "999".into()).unwrap();
}

#[test]
fn a_user_with_read_only_rights_is_told_so() {
    let rig = Rig::new();
    let shared = Some(SHARED_ACCOUNT.to_string());
    for words in [
        rig.core.file_rename(shared.clone(), "3".into(), "x.txt".into()).unwrap_err(),
        rig.core.file_delete(shared.clone(), "3".into()).unwrap_err(),
        rig.core.file_create_folder(shared.clone(), s("2"), "x".into()).map(|_| ()).unwrap_err(),
        rig.core.file_create_folder(shared.clone(), None, "x".into()).map(|_| ()).unwrap_err(),
    ] {
        assert!(invalid(words).contains("not allowed"));
    }
    assert_eq!(rig.server.file_nodes(SHARED_ACCOUNT).len(), 3);
    // Where the folder allows it, it works, in the owner's space.
    let made = rig.core.file_create_folder(shared, s("1"), "Mine".into()).unwrap();
    assert_eq!(made.account_id, s(SHARED_ACCOUNT));
    assert_eq!(rig.server.file_nodes(SHARED_ACCOUNT).len(), 4);
}

#[test]
fn quota_errors_are_told_in_three_ways() {
    let rig = Rig::new();
    for (description, word) in [
        ("There are too many folders, please delete some before adding a new one.", "folders"),
        ("There are too many files, please delete some before adding a new one.", "files"),
        ("The disk quota has been exceeded.", "full"),
    ] {
        rig.server.fail_next_file_set("overQuota", description);
        let words = invalid(rig.core.file_create_folder(None, None, "x".into()).unwrap_err());
        assert!(words.contains(word) && !words.contains("quota has"), "{words}");
    }
    rig.server.fail_next_file_set("somethingNew", "odd");
    let words = invalid(rig.core.file_create_folder(None, None, "x".into()).unwrap_err());
    assert!(words.contains("somethingNew"));
    assert!(rig.server.file_nodes(ACCOUNT_ID).is_empty());
}

// ---- upload ----

#[test]
fn an_upload_streams_the_file_and_leaves_the_source_alone() {
    let rig = Rig::new();
    // Larger than one piece of the stream, with content that shows a lost or doubled piece.
    let bytes: Vec<u8> = (0..3_000_000_u32).map(|i| (i % 251) as u8).collect();
    let source = rig.local("big.bin", &bytes);
    let before = fs::metadata(&source).unwrap().modified().unwrap();
    let entry = rig.upload(None, None, "big.bin", &source, false).unwrap();
    assert_eq!((entry.name.as_str(), entry.kind, entry.size), ("big.bin", FileKind::File, 3_000_000));
    assert_eq!(entry.media_type, "text/plain");
    assert!(entry.blob_id.is_some() && entry.may_replace);
    assert_eq!(rig.server.file_content(ACCOUNT_ID, &entry.id).unwrap(), bytes);
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(fs::metadata(&source).unwrap().modified().unwrap(), before);
    assert_eq!(rig.server.file_uploads(), [(ACCOUNT_ID.to_string(), 3_000_000)]);
}

#[test]
fn an_empty_media_type_is_octet_stream_and_an_empty_file_is_fine() {
    let rig = Rig::new();
    let source = rig.local("empty", b"");
    let entry = rig
        .core
        .file_upload(None, None, "empty".into(), String::new(), source.to_string_lossy().into_owned(), false)
        .unwrap();
    assert_eq!((entry.size, entry.media_type.as_str()), (0, "application/octet-stream"));
}

#[test]
fn an_upload_into_a_shared_space_goes_to_the_own_upload_url() {
    let rig = Rig::new();
    let source = rig.local("note.txt", b"for the owner");
    let entry = rig.upload(s(SHARED_ACCOUNT).as_deref(), s("1").as_deref(), "note.txt", &source, false).unwrap();
    assert_eq!(entry.account_id, s(SHARED_ACCOUNT));
    // The bytes went to the user's own account; the node lives in the shared one.
    assert_eq!(rig.server.file_uploads(), [(ACCOUNT_ID.to_string(), 13)]);
    assert_eq!(rig.server.file_content(SHARED_ACCOUNT, &entry.id).unwrap(), b"for the owner");
    assert!(rig.server.file_nodes(ACCOUNT_ID).is_empty());
    assert_eq!(rig.names(s(SHARED_ACCOUNT).as_deref(), s("1").as_deref()), ["ReadOnly", "hello.txt", "note.txt"]);
    // Not where the folder does not let the user add.
    let again = rig.upload(s(SHARED_ACCOUNT).as_deref(), s("2").as_deref(), "note.txt", &source, false);
    assert!(invalid(again.unwrap_err()).contains("not allowed"));
}

#[test]
fn a_name_that_is_taken_is_replaced_only_when_asked_and_only_a_file() {
    let rig = Rig::new();
    let folder = rig.server.add_folder(ACCOUNT_ID, None, "dir", None);
    let existing = rig.server.add_file(ACCOUNT_ID, None, "a.txt", b"old", "text/plain");
    rig.server.set_file_modified(ACCOUNT_ID, &existing, "2021-01-01T00:00:00Z");
    let source = rig.local("new.txt", b"the new content");

    let words = invalid(rig.upload(None, None, "a.txt", &source, false).unwrap_err());
    assert!(words.contains("already there"));
    assert_eq!(rig.server.file_content(ACCOUNT_ID, &existing).unwrap(), b"old");
    assert_eq!(rig.server.file_nodes(ACCOUNT_ID).len(), 2);

    let _ = folder;
    let replaced = rig.upload(None, None, "a.txt", &source, true).unwrap();
    assert_eq!((replaced.id.as_str(), replaced.size), (existing.as_str(), 15));
    assert_eq!(rig.server.file_content(ACCOUNT_ID, &existing).unwrap(), b"the new content");
    // The server does not bump the time; the core does.
    let at = replaced.modified.unwrap();
    assert!((now() - at).abs() < 60, "modified {at}, now {}", now());
    assert_eq!(rig.server.file_nodes(ACCOUNT_ID).len(), 2);

    // A folder of that name is never replaced.
    let words = invalid(rig.upload(None, None, "dir", &source, true).unwrap_err());
    assert!(words.contains("folder"));
    assert_eq!(rig.server.file_nodes(ACCOUNT_ID).len(), 2);
}

#[test]
fn a_taken_name_is_found_before_anything_is_uploaded() {
    let rig = Rig::new();
    let existing = rig.server.add_file(ACCOUNT_ID, None, "a.txt", b"old", "text/plain");
    let source = rig.local("new.txt", b"the new content");
    // Without replace: refused, and not one byte was sent.
    assert!(invalid(rig.upload(None, None, "a.txt", &source, false).unwrap_err()).contains("already there"));
    assert!(rig.server.file_uploads().is_empty());
    // The same, in a folder.
    let dir = rig.server.add_folder(ACCOUNT_ID, None, "dir", None);
    rig.server.add_file(ACCOUNT_ID, Some(&dir), "b.txt", b"old", "text/plain");
    assert!(invalid(rig.upload(None, Some(&dir), "b.txt", &source, false).unwrap_err()).contains("already there"));
    let words = invalid(rig.upload(None, None, "dir", &source, true).unwrap_err());
    assert!(words.contains("folder"));
    assert!(rig.server.file_uploads().is_empty());
    // The confirmed replace is the one and only upload.
    rig.upload(None, None, "a.txt", &source, true).unwrap();
    assert_eq!(rig.server.file_uploads().len(), 1);
    assert_eq!(rig.server.file_content(ACCOUNT_ID, &existing).unwrap(), b"the new content");
}

#[test]
fn a_replace_gives_the_new_media_type() {
    let rig = Rig::new();
    let existing = rig.server.add_file(ACCOUNT_ID, None, "doc", b"old", "application/octet-stream");
    let source = rig.local("doc.pdf", b"%PDF new");
    let entry = rig
        .core
        .file_upload(None, None, "doc".into(), "application/pdf".into(), source.to_string_lossy().into_owned(), true)
        .unwrap();
    assert_eq!((entry.id.as_str(), entry.media_type.as_str()), (existing.as_str(), "application/pdf"));
}

#[test]
fn a_file_can_be_replaced_in_a_folder_that_allows_changing_content() {
    let rig = Rig::new();
    let source = rig.local("new.txt", b"better");
    let entry = rig.upload(s(SHARED_ACCOUNT).as_deref(), s("1").as_deref(), "hello.txt", &source, true).unwrap();
    assert_eq!((entry.id.as_str(), entry.size, entry.account_id.clone()), ("3", 6, s(SHARED_ACCOUNT)));
    assert_eq!(rig.server.file_content(SHARED_ACCOUNT, "3").unwrap(), b"better");
}

#[test]
fn the_uploads_budget_is_a_temporary_error_that_says_for_how_long() {
    let rig = Rig::new();
    let source = rig.local("a.txt", b"x");
    rig.server.refuse_next_upload_with_429(213);
    match rig.upload(None, None, "a.txt", &source, false).unwrap_err() {
        Error::Temporary { msg } => assert!(msg.contains("4 minutes"), "{msg}"),
        other => panic!("expected Temporary, got {other:?}"),
    }
    assert!(rig.server.file_nodes(ACCOUNT_ID).is_empty());
    // And it works when the budget is back.
    rig.upload(None, None, "a.txt", &source, false).unwrap();
}

#[test]
fn a_file_larger_than_the_server_takes_sends_nothing() {
    let rig = Rig::with(|server| server.set_upload_limit(10));
    assert_eq!(rig.core.file_spaces().unwrap()[0].max_upload, 10);
    let source = rig.local("big.txt", b"more than ten bytes");
    let words = invalid(rig.upload(None, None, "big.txt", &source, false).unwrap_err());
    assert!(words.contains("takes at most"), "{words}");
    assert!(rig.server.file_uploads().is_empty());
    assert!(rig.server.file_nodes(ACCOUNT_ID).is_empty());
    // One that fits goes through.
    let small = rig.local("small.txt", b"ten bytes!");
    rig.upload(None, None, "small.txt", &small, false).unwrap();
}

#[test]
fn a_missing_or_wrong_source_is_invalid_and_sends_nothing() {
    let rig = Rig::new();
    let missing = rig.dir.path().join("nope.txt");
    assert!(invalid(rig.upload(None, None, "x", &missing, false).unwrap_err()).contains("cannot be read"));
    assert!(invalid(rig.upload(None, None, "x", rig.dir.path(), false).unwrap_err()).contains("not a file"));
    assert!(rig.server.file_uploads().is_empty());
}

#[test]
fn a_full_quota_on_upload_is_invalid_with_words() {
    let rig = Rig::new();
    let source = rig.local("a.txt", b"x");
    rig.server.fail_next_file_set("overQuota", "The disk quota has been exceeded.");
    assert!(invalid(rig.upload(None, None, "a.txt", &source, false).unwrap_err()).contains("full"));
}

// ---- download ----

#[test]
fn a_download_streams_to_the_destination_through_a_part_file() {
    let rig = Rig::new();
    let bytes: Vec<u8> = (0..300_000_u32).map(|i| (i % 253) as u8).collect();
    let id = rig.server.add_file(ACCOUNT_ID, None, "d.bin", &bytes, "application/octet-stream");
    let entry = rig.core.files(None, None).unwrap().entries.into_iter().find(|e| e.id == id).unwrap();
    let dest = rig.dir.path().join("out.bin");
    rig.core.file_download(None, entry.blob_id.clone().unwrap(), dest.to_string_lossy().into_owned()).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), bytes);
    assert_eq!(rig.server.file_downloads(), [ACCOUNT_ID.to_string()]);
    let leftovers: Vec<_> = fs::read_dir(rig.dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn a_download_from_a_shared_space_names_that_space_in_the_url() {
    let rig = Rig::new();
    let listing = rig.core.files(s(SHARED_ACCOUNT), s("1")).unwrap();
    let hello = listing.entries.iter().find(|e| e.name == "hello.txt").unwrap();
    let dest = rig.dir.path().join("hello.txt");
    rig.core
        .file_download(s(SHARED_ACCOUNT), hello.blob_id.clone().unwrap(), dest.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"hello");
    assert_eq!(rig.server.file_downloads(), [SHARED_ACCOUNT.to_string()]);
}

#[test]
fn a_failed_download_leaves_no_file_and_no_part_file() {
    let rig = Rig::new();
    let dest = rig.dir.path().join("never.bin");
    let e = rig.core.file_download(None, "no-such-blob".into(), dest.to_string_lossy().into_owned()).unwrap_err();
    assert!(invalid(e).contains("does not have"));
    let names: Vec<String> = fs::read_dir(rig.dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with("never")), "{names:?}");
    assert!(rig.core.file_download(s("nobody"), "x".into(), dest.to_string_lossy().into_owned()).is_err());
}
