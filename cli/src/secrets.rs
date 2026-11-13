//! The `SecretStore` of the cli: a JSON object in a file.
//!
//! This keeps the OAuth refresh token UNPROTECTED on disk (mode 0600 on Unix, nothing special
//! on Windows). That is acceptable for a developer tool and for nothing else; the Android app
//! uses the Keystore instead.
//!
//! The file is re-read on every call, so it always reflects what is on disk and two `kweb`
//! processes cannot hold stale copies. Writes go to a temp file that is renamed over the
//! target, so a crash never leaves half a file.

use std::{collections::BTreeMap, fs, io, io::Write, path::Path, path::PathBuf, sync::Mutex};

use kweb_core::secrets::{SecretError, SecretStore};

pub struct FileSecrets {
    path: PathBuf,
    /// Serializes read-modify-write within this process.
    write: Mutex<()>,
}

type Map = BTreeMap<String, String>;

impl FileSecrets {
    pub fn new(path: PathBuf) -> Self {
        FileSecrets { path, write: Mutex::new(()) }
    }

    fn read(&self) -> Result<Map, SecretError> {
        match fs::read_to_string(&self.path) {
            Ok(text) if text.trim().is_empty() => Ok(Map::new()),
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| failed(format!("{} is not valid: {e}", self.path.display())))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(failed(format!("reading {}: {e}", self.path.display()))),
        }
    }

    fn write(&self, map: &Map) -> Result<(), SecretError> {
        let text = serde_json::to_string_pretty(map).map_err(|e| failed(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        write_private(&tmp, text.as_bytes()).map_err(|e| failed(format!("writing {}: {e}", tmp.display())))?;
        fs::rename(&tmp, &self.path).map_err(|e| failed(format!("replacing {}: {e}", self.path.display())))
    }

    fn change(&self, edit: impl FnOnce(&mut Map)) -> Result<(), SecretError> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.read()?;
        edit(&mut map);
        self.write(&map)
    }
}

impl SecretStore for FileSecrets {
    fn get(&self, key: String) -> Result<Option<String>, SecretError> {
        Ok(self.read()?.remove(&key))
    }

    fn set(&self, key: String, value: String) -> Result<(), SecretError> {
        self.change(|map| {
            map.insert(key, value);
        })
    }

    fn delete(&self, key: String) -> Result<(), SecretError> {
        self.change(|map| {
            map.remove(&key);
        })
    }
}

fn failed(msg: String) -> SecretError {
    SecretError::Failed { msg }
}

/// Creates (or truncates) the file readable by its owner only where the platform can say so.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that removes itself, so the cli needs no dev-dependency for it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("kweb-cli-{name}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("secrets.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_missing_file_is_empty() {
        let dir = Scratch::new("missing");
        let store = FileSecrets::new(dir.file());
        assert_eq!(store.get("k".into()), Ok(None));
        store.delete("k".into()).unwrap();
    }

    #[test]
    fn round_trip_and_overwrite() {
        let dir = Scratch::new("round");
        let store = FileSecrets::new(dir.file());
        store.set("a".into(), "1".into()).unwrap();
        store.set("b".into(), "2".into()).unwrap();
        store.set("a".into(), "3".into()).unwrap();
        assert_eq!(store.get("a".into()), Ok(Some("3".into())));
        assert_eq!(store.get("b".into()), Ok(Some("2".into())));
        assert_eq!(store.get("c".into()), Ok(None));
    }

    #[test]
    fn delete_removes_only_that_key() {
        let dir = Scratch::new("delete");
        let store = FileSecrets::new(dir.file());
        store.set("a".into(), "1".into()).unwrap();
        store.set("b".into(), "2".into()).unwrap();
        store.delete("a".into()).unwrap();
        assert_eq!(store.get("a".into()), Ok(None));
        assert_eq!(store.get("b".into()), Ok(Some("2".into())));
    }

    #[test]
    fn survives_reopen_and_leaves_no_temp_file() {
        let dir = Scratch::new("reopen");
        FileSecrets::new(dir.file()).set("token".into(), "s3cret \"quoted\"".into()).unwrap();
        let again = FileSecrets::new(dir.file());
        assert_eq!(again.get("token".into()), Ok(Some("s3cret \"quoted\"".into())));
        assert!(!dir.0.join("secrets.json.tmp").exists());
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_silently_empty() {
        let dir = Scratch::new("corrupt");
        fs::write(dir.file(), "{ not json").unwrap();
        let store = FileSecrets::new(dir.file());
        assert!(store.get("k".into()).is_err());
        // And it is not overwritten behind the user's back.
        assert!(store.set("k".into(), "v".into()).is_err());
        assert_eq!(fs::read_to_string(dir.file()).unwrap(), "{ not json");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("mode");
        FileSecrets::new(dir.file()).set("k".into(), "v".into()).unwrap();
        assert_eq!(fs::metadata(dir.file()).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
