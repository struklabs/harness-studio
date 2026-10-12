use crate::error_key::err;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static PATH_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

/// One lock per file. The key is the canonical *directory* plus the file name: the target file
/// itself is never opened here, because on Windows an open handle on it (even a read-only one
/// taken by a concurrent caller) can make another caller's atomic replace of that file fail.
fn path_lock(path: &Path) -> Arc<Mutex<()>> {
    let name = path.file_name().unwrap_or_default();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // A directory that does not exist yet (first save) cannot be canonicalized; walk up to the
    // nearest one that does so the key stays the same before and after it is created.
    let mut existing = parent;
    let mut tail = Vec::new();
    let canonical = loop {
        match fs::canonicalize(existing) {
            Ok(dir) => break Some(dir),
            Err(_) => match (existing.parent(), existing.file_name()) {
                (Some(up), Some(part)) => {
                    tail.push(part);
                    existing = up;
                }
                _ => break None,
            },
        }
    };
    let key = match canonical {
        Some(mut dir) => {
            dir.extend(tail.iter().rev());
            dir.join(name)
        }
        None => path.to_path_buf(),
    };
    PATH_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[derive(Debug, Clone)]
pub struct JsonStore {
    path: PathBuf,
    sensitive: bool,
}

impl JsonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            sensitive: false,
        }
    }

    pub fn sensitive(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            sensitive: true,
        }
    }

    pub fn load_or_default<T: DeserializeOwned + Default>(&self) -> T {
        self.load().unwrap_or_default()
    }

    pub fn load<T: DeserializeOwned>(&self) -> Result<T, String> {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        let content = fs::read_to_string(&self.path).map_err(|e| self.read_error(e))?;
        serde_json::from_str(&content).map_err(|e| self.parse_error(e))
    }

    fn read_error(&self, e: impl std::fmt::Display) -> String {
        err("fs.read_failed")
            .with("path", self.path.display())
            .with("error", e)
            .into()
    }

    fn write_error(&self, e: impl std::fmt::Display) -> String {
        err("fs.write_failed")
            .with("path", self.path.display())
            .with("error", e)
            .into()
    }

    fn parse_error(&self, e: impl std::fmt::Display) -> String {
        err("json.parse_failed")
            .with("path", self.path.display())
            .with("error", e)
            .into()
    }

    pub fn save<T: Serialize + ?Sized>(&self, value: &T) -> Result<(), String> {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        self.save_locked(value)
    }

    pub fn update<T, R>(
        &self,
        update: impl FnOnce(&mut T) -> Result<R, String>,
    ) -> Result<R, String>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        let mut value = match fs::read_to_string(&self.path) {
            Ok(content) => serde_json::from_str(&content).map_err(|e| self.parse_error(e))?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => T::default(),
            Err(e) => return Err(self.read_error(e)),
        };
        let result = update(&mut value)?;
        self.save_locked(&value)?;
        Ok(result)
    }

    fn save_locked<T: Serialize + ?Sized>(&self, value: &T) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(value).map_err(serialize_error)?;
        self.write_locked(&data).map_err(|e| self.write_error(e))
    }

    pub fn write_bytes(&self, data: &[u8]) -> Result<(), String> {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        self.write_locked(data).map_err(|e| self.write_error(e))
    }

    fn write_locked(&self, data: &[u8]) -> io::Result<()> {
        let _sensitive = self.sensitive;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let name = self
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("data");
        let (temp, mut file) = loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let temp = parent.join(format!(".{}.{}.{}.tmp", name, std::process::id(), id));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            if self.sensitive {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temp) {
                Ok(file) => break (temp, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let result = (|| {
            file.write_all(data)?;
            file.flush()?;
            file.sync_all()?;
            drop(file);
            replace_file_retrying(&temp, &self.path)?;
            #[cfg(unix)]
            if self.sensitive {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
            }
            if let Ok(dir) = fs::File::open(parent) {
                let _ = dir.sync_all();
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

// --- Versioned files (schema_version + ordered migrations) ---

/// i18n key of the error returned when a file was written by a newer Harness Studio.
pub const NEWER_VERSION_KEY: &str = "store.newer_version";

fn serialize_error(e: serde_json::Error) -> String {
    err("json.serialize_failed").with("error", e).into()
}

/// Where the data sits in a versioned document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Envelope {
    /// The data is a JSON object; `schema_version` is one more field of it
    /// (`{"schema_version": 1, "theme": "dark", ...}`). Older builds ignore the field.
    Merged,
    /// The data sits under this key (`{"schema_version": 2, "jobs": [...]}`); for files whose
    /// data is an array or a free-form map.
    Wrapped(&'static str),
}

/// One migration step `v(N) -> v(N+1)` on the whole document. The framework sets
/// `schema_version` after each step, so a step only reshapes the data.
pub type MigrationFn = fn(serde_json::Value) -> Result<serde_json::Value, String>;

/// Schema of a versioned JSON file.
#[derive(Debug, Clone, Copy)]
pub struct Schema {
    /// File label for messages (`"downloads.json"`).
    pub file: &'static str,
    /// Version this build writes.
    pub current: u32,
    pub envelope: Envelope,
    /// `migrations[i]` turns version `i + 1` into `i + 2`; `len() == current - 1`.
    pub migrations: &'static [MigrationFn],
}

/// A loaded versioned file.
#[derive(Debug, Clone, PartialEq)]
pub struct Versioned<T> {
    pub data: T,
    /// Version on disk (after a migration: `schema.current`; a newer file keeps its own).
    pub version: u32,
    /// Written by a newer build: the data is usable but every write is refused so the newer
    /// file is never overwritten.
    pub read_only: bool,
}

/// Version of a parsed document; a document without `schema_version` is legacy version 1.
pub fn document_version(doc: &serde_json::Value) -> u32 {
    doc.get("schema_version")
        .and_then(|v| v.as_u64())
        .map(|v| v.min(u32::MAX as u64) as u32)
        .unwrap_or(1)
}

/// `[store.newer_version] ...` (the key lets the UI localize it; the rest is readable as is).
pub fn newer_version_error(file: &str, found: u32, supported: u32) -> String {
    format!(
        "[{}] {} daha yeni bir Harness Studio sürümüyle yazılmış (şema {}, bu sürüm en fazla {}). Dosya korunuyor; değişiklik kaydedilmedi.",
        NEWER_VERSION_KEY, file, found, supported
    )
}

/// The data part of a document (without the version / envelope).
fn doc_payload(schema: &Schema, mut doc: serde_json::Value) -> serde_json::Value {
    match schema.envelope {
        Envelope::Merged => {
            if let Some(map) = doc.as_object_mut() {
                map.remove("schema_version");
            }
            doc
        }
        Envelope::Wrapped(key) => doc
            .as_object_mut()
            .and_then(|m| m.remove(key))
            .unwrap_or(serde_json::Value::Null),
    }
}

fn parse_payload<T: DeserializeOwned + Default>(
    schema: &Schema,
    doc: serde_json::Value,
) -> Result<T, String> {
    let value = doc_payload(schema, doc);
    if value.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(value).map_err(|e| {
        err("json.parse_failed")
            .with("path", schema.file)
            .with("error", e)
            .into()
    })
}

fn envelope_document(
    schema: &Schema,
    data: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut out = serde_json::Map::new();
    out.insert("schema_version".into(), schema.current.into());
    match schema.envelope {
        Envelope::Merged => match data {
            serde_json::Value::Object(map) => {
                out.extend(map.into_iter().filter(|(k, _)| k != "schema_version"));
            }
            _ => return Err(err("store.not_object").with("file", schema.file).into()),
        },
        Envelope::Wrapped(key) => {
            out.insert(key.into(), data);
        }
    }
    Ok(serde_json::Value::Object(out))
}

/// Migration helper: puts a legacy (unversioned) document under `key`.
pub fn wrap_legacy(doc: serde_json::Value, key: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(key.to_string(), doc);
    serde_json::Value::Object(map)
}

impl JsonStore {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `<file>.bak-v<N>` next to the file.
    pub fn backup_path(&self, version: u32) -> PathBuf {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.path
            .with_file_name(format!("{}.bak-v{}", name, version))
    }

    /// Copies the file to `<file>.bak-v<N>` (an existing backup of that version is kept).
    fn backup_locked(&self, version: u32) -> Result<(), String> {
        let backup = self.backup_path(version);
        if backup.exists() {
            return Ok(());
        }
        let data = fs::read(&self.path).map_err(|e| self.read_error(e))?;
        JsonStore {
            path: backup,
            sensitive: self.sensitive,
        }
        .write_locked(&data)
        .map_err(|e| {
            err("store.backup_failed")
                .with("path", self.path.display())
                .with("error", e)
                .into()
        })
    }

    /// Reads, migrates (backup + atomic rewrite) and parses a versioned file. `Ok(None)` = no
    /// file. A newer file is returned read-only and never touched. A file that doesn't parse
    /// is an error and is left as it is.
    pub fn load_versioned<T: DeserializeOwned + Default>(
        &self,
        schema: &Schema,
    ) -> Result<Option<Versioned<T>>, String> {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        self.load_versioned_locked(schema)
    }

    /// [`load_versioned`](Self::load_versioned) with a missing file as default data.
    pub fn load_versioned_or_default<T: DeserializeOwned + Default>(
        &self,
        schema: &Schema,
    ) -> Result<Versioned<T>, String> {
        Ok(self.load_versioned(schema)?.unwrap_or(Versioned {
            data: T::default(),
            version: schema.current,
            read_only: false,
        }))
    }

    fn load_versioned_locked<T: DeserializeOwned + Default>(
        &self,
        schema: &Schema,
    ) -> Result<Option<Versioned<T>>, String> {
        debug_assert_eq!(schema.migrations.len() + 1, schema.current as usize);
        let content = match fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(self.read_error(e)),
        };
        let mut doc: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| self.parse_error(e))?;
        let found = document_version(&doc);
        if found > schema.current {
            return Ok(Some(Versioned {
                data: parse_payload(schema, doc)?,
                version: found,
                read_only: true,
            }));
        }
        if found < schema.current {
            // Everything happens in memory first: a failing step leaves the file untouched.
            let mut version = found.max(1);
            while version < schema.current {
                let step = schema.migrations[(version - 1) as usize];
                doc = step(doc).map_err(|e| {
                    String::from(
                        err("store.migration_failed")
                            .with("file", schema.file)
                            .num("from", version)
                            .num("to", version + 1)
                            .with("error", e),
                    )
                })?;
                version += 1;
                if let Some(map) = doc.as_object_mut() {
                    map.insert("schema_version".into(), version.into());
                }
            }
            let data: T = parse_payload(schema, doc.clone())?;
            let bytes =
                serde_json::to_vec_pretty(&envelope_document(schema, doc_payload(schema, doc))?)
                    .map_err(serialize_error)?;
            self.backup_locked(found)?;
            self.write_locked(&bytes).map_err(|e| self.write_error(e))?;
            return Ok(Some(Versioned {
                data,
                version: schema.current,
                read_only: false,
            }));
        }
        Ok(Some(Versioned {
            data: parse_payload(schema, doc)?,
            version: found,
            read_only: false,
        }))
    }

    /// Version of the file on disk (`None`: missing or not JSON).
    fn disk_version_locked(&self) -> Option<u32> {
        let content = fs::read_to_string(&self.path).ok()?;
        let doc: serde_json::Value = serde_json::from_str(&content).ok()?;
        Some(document_version(&doc))
    }

    /// Writes `value` as the current version (atomically). Refused when the file on disk is
    /// newer; an older file is backed up first.
    pub fn save_versioned<T: Serialize + ?Sized>(
        &self,
        schema: &Schema,
        value: &T,
    ) -> Result<(), String> {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        self.save_versioned_locked(schema, value)
    }

    fn save_versioned_locked<T: Serialize + ?Sized>(
        &self,
        schema: &Schema,
        value: &T,
    ) -> Result<(), String> {
        match self.disk_version_locked() {
            Some(v) if v > schema.current => {
                return Err(newer_version_error(schema.file, v, schema.current))
            }
            Some(v) if v < schema.current => self.backup_locked(v)?,
            _ => {}
        }
        let data = serde_json::to_value(value).map_err(serialize_error)?;
        let bytes = serde_json::to_vec_pretty(&envelope_document(schema, data)?)
            .map_err(serialize_error)?;
        self.write_locked(&bytes).map_err(|e| self.write_error(e))
    }

    /// Read-modify-write of a versioned file under the path lock (migrating it first).
    pub fn update_versioned<T, R>(
        &self,
        schema: &Schema,
        update: impl FnOnce(&mut T) -> Result<R, String>,
    ) -> Result<R, String>
    where
        T: Serialize + DeserializeOwned + Default,
    {
        let lock = path_lock(&self.path);
        let _guard = lock.lock().unwrap();
        let loaded = self.load_versioned_locked::<T>(schema)?;
        if let Some(l) = loaded.as_ref().filter(|l| l.read_only) {
            return Err(newer_version_error(schema.file, l.version, schema.current));
        }
        let mut value = loaded.map(|l| l.data).unwrap_or_default();
        let result = update(&mut value)?;
        self.save_versioned_locked(schema, &value)?;
        Ok(result)
    }
}

/// Windows: a virus scanner or the search indexer may hold the destination open for a moment,
/// which makes the replace fail with access denied / sharing violation. Those clear on their
/// own, so retry briefly before giving up.
fn replace_file_retrying(source: &Path, destination: &Path) -> io::Result<()> {
    let mut wait_ms = 2;
    for _ in 0..8 {
        match replace_file(source, destination) {
            Err(e) if is_transient_replace_error(&e) => {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                wait_ms = (wait_ms * 2).min(100);
            }
            other => return other,
        }
    }
    replace_file(source, destination)
}

fn is_transient_replace_error(e: &io::Error) -> bool {
    // 5 = ERROR_ACCESS_DENIED, 32 = ERROR_SHARING_VIOLATION, 33 = ERROR_LOCK_VIOLATION.
    cfg!(windows)
        && (e.kind() == io::ErrorKind::PermissionDenied
            || matches!(e.raw_os_error(), Some(5 | 32 | 33)))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(win32_io_error)
    }
}

/// `io::Error::from(windows::core::Error)` keeps the HRESULT (`0x80070005` for access denied) as
/// the OS error code, so its kind is `Uncategorized` and the Win32 checks above never matched:
/// the retry never ran. Win32 failures (FACILITY_WIN32, `0x8007xxxx`) go back to their Win32
/// code; any other HRESULT is kept as is.
#[cfg(windows)]
fn win32_io_error(e: windows::core::Error) -> io::Error {
    let code = e.code().0 as u32;
    if code & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((code & 0xFFFF) as i32)
    } else {
        io::Error::from(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn win32_hresults_become_win32_errors_and_count_as_transient() {
        use windows::core::{Error, HRESULT};
        let denied = win32_io_error(Error::from_hresult(HRESULT(0x8007_0005_u32 as i32)));
        assert_eq!(denied.raw_os_error(), Some(5));
        assert_eq!(denied.kind(), io::ErrorKind::PermissionDenied);
        assert!(is_transient_replace_error(&denied));
        let sharing = win32_io_error(Error::from_hresult(HRESULT(0x8007_0020_u32 as i32)));
        assert_eq!(sharing.raw_os_error(), Some(32));
        assert!(is_transient_replace_error(&sharing));
        // Not FACILITY_WIN32 (E_FAIL): kept as is, not retried.
        let other = win32_io_error(Error::from_hresult(HRESULT(0x8000_4005_u32 as i32)));
        assert_eq!(other.raw_os_error(), Some(0x8000_4005_u32 as i32));
        assert!(!is_transient_replace_error(&other));
    }

    /// A scanner holding the destination open without sharing makes the replace fail with
    /// access denied; the save must wait for it instead of failing. The holder lets go after
    /// 30 ms, well inside the ~330 ms retry window.
    #[cfg(windows)]
    #[test]
    fn save_waits_out_a_briefly_locked_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let store = JsonStore::new(&path);
        store.save(&vec![1]).unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            locked_rx.recv().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
            drop(held);
        });
        // Without the retry this is the error a scanner causes.
        let tmp = dir.path().join("probe.tmp");
        fs::write(&tmp, b"[2]").unwrap();
        let err = replace_file(&tmp, &path).unwrap_err();
        assert!(is_transient_replace_error(&err), "{err:?}");
        fs::remove_file(&tmp).unwrap();
        locked_tx.send(()).unwrap();
        store.save(&vec![2]).unwrap();
        holder.join().unwrap();
        assert_eq!(store.load::<Vec<u8>>().unwrap(), vec![2]);
    }

    #[test]
    fn replaces_json_without_leaving_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let store = JsonStore::new(&path);
        store.save(&vec![1, 2]).unwrap();
        store.save(&vec![3]).unwrap();
        assert_eq!(store.load::<Vec<u8>>().unwrap(), vec![3]);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// Readers and writers of one path hit each other constantly; every call must succeed.
    /// (Regression: looking the lock up used to open the target file itself, which on Windows
    /// made a concurrent replace of that file fail with "access denied".)
    #[test]
    fn concurrent_load_save_update_never_fail() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(JsonStore::new(dir.path().join("mixed.json")));
        store.save(&0u64).unwrap();
        let threads: Vec<_> = (0..16)
            .map(|n| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        if n % 2 == 0 {
                            store
                                .update::<u64, _>(|v| {
                                    *v += 1;
                                    Ok(())
                                })
                                .unwrap();
                        } else {
                            store.load::<u64>().unwrap();
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(store.load::<u64>().unwrap(), 800);
    }

    #[test]
    fn update_serializes_read_modify_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(JsonStore::new(dir.path().join("counter.json")));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .update::<u64, _>(|n| {
                            *n += 1;
                            Ok(())
                        })
                        .unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(store.load::<u64>().unwrap(), 8);
    }

    // --- versioned files ---

    fn wrap_items(doc: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(wrap_legacy(doc, "items"))
    }

    /// v2 -> v3: every item gets doubled (proves steps run in order on the output of the last).
    fn double_items(mut doc: serde_json::Value) -> Result<serde_json::Value, String> {
        let items = doc["items"].as_array_mut().ok_or("items yok")?;
        for v in items.iter_mut() {
            *v = (v.as_u64().ok_or("sayı değil")? * 2).into();
        }
        Ok(doc)
    }

    fn fail_step(_: serde_json::Value) -> Result<serde_json::Value, String> {
        Err("bilerek başarısız".into())
    }

    const LIST_V2: Schema = Schema {
        file: "list.json",
        current: 2,
        envelope: Envelope::Wrapped("items"),
        migrations: &[wrap_items],
    };
    const LIST_V3: Schema = Schema {
        file: "list.json",
        current: 3,
        envelope: Envelope::Wrapped("items"),
        migrations: &[wrap_items, double_items],
    };

    #[derive(Debug, Default, PartialEq, Serialize, serde::Deserialize)]
    struct Obj {
        #[serde(default)]
        theme: String,
    }
    const OBJ_V1: Schema = Schema {
        file: "obj.json",
        current: 1,
        envelope: Envelope::Merged,
        migrations: &[],
    };

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn legacy_file_without_version_is_v1_migrated_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        fs::write(&path, "[1,2,3]").unwrap();
        let store = JsonStore::new(&path);

        let loaded = store.load_versioned::<Vec<u64>>(&LIST_V3).unwrap().unwrap();
        assert_eq!(loaded.data, vec![2, 4, 6]);
        assert_eq!((loaded.version, loaded.read_only), (3, false));
        // Backup is the untouched original; the file itself is the new version.
        assert_eq!(fs::read_to_string(store.backup_path(1)).unwrap(), "[1,2,3]");
        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc,
            serde_json::json!({"schema_version": 3, "items": [2, 4, 6]})
        );
        // Atomic writes leave no temp files behind.
        assert_eq!(files_in(dir.path()), vec!["list.json", "list.json.bak-v1"]);
        // Loading again is a no-op.
        let again = store.load_versioned::<Vec<u64>>(&LIST_V3).unwrap().unwrap();
        assert_eq!(again.data, vec![2, 4, 6]);
        assert_eq!(files_in(dir.path()).len(), 2);
    }

    #[test]
    fn migration_chain_starts_at_the_version_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        fs::write(&path, r#"{"schema_version":2,"items":[5]}"#).unwrap();
        let store = JsonStore::new(&path);
        let loaded = store.load_versioned::<Vec<u64>>(&LIST_V3).unwrap().unwrap();
        assert_eq!(loaded.data, vec![10]);
        assert!(store.backup_path(2).exists());
        assert!(!store.backup_path(1).exists());
    }

    #[test]
    fn merged_legacy_object_is_current_v1_and_save_adds_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obj.json");
        fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let store = JsonStore::new(&path);
        let loaded = store.load_versioned::<Obj>(&OBJ_V1).unwrap().unwrap();
        assert_eq!(loaded.data.theme, "dark");
        // v1 == current: no backup, file untouched until the next save.
        assert_eq!(files_in(dir.path()), vec!["obj.json"]);
        store.save_versioned(&OBJ_V1, &loaded.data).unwrap();
        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc,
            serde_json::json!({"schema_version": 1, "theme": "dark"})
        );
        // `schema_version` comes first so a human sees it at the top.
        assert!(fs::read_to_string(&path)
            .unwrap()
            .trim_start_matches(['{', '\n', ' '])
            .starts_with("\"schema_version\""));
    }

    #[test]
    fn future_version_is_read_only_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        let newer = r#"{"schema_version":9,"items":[7],"unknown_future_field":true}"#;
        fs::write(&path, newer).unwrap();
        let store = JsonStore::new(&path);

        let loaded = store.load_versioned::<Vec<u64>>(&LIST_V2).unwrap().unwrap();
        assert_eq!(loaded.data, vec![7]);
        assert_eq!((loaded.version, loaded.read_only), (9, true));

        let err = store.save_versioned(&LIST_V2, &vec![1u64]).unwrap_err();
        assert!(err.starts_with("[store.newer_version]"), "{err}");
        let err = store
            .update_versioned::<Vec<u64>, _>(&LIST_V2, |v| {
                v.push(1);
                Ok(())
            })
            .unwrap_err();
        assert!(err.contains(NEWER_VERSION_KEY));
        assert_eq!(fs::read_to_string(&path).unwrap(), newer);
        assert_eq!(files_in(dir.path()), vec!["list.json"]);
    }

    #[test]
    fn failing_migration_or_bad_json_leaves_the_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        fs::write(&path, "[1]").unwrap();
        let broken = Schema {
            file: "list.json",
            current: 2,
            envelope: Envelope::Wrapped("items"),
            migrations: &[fail_step],
        };
        let store = JsonStore::new(&path);
        let err = store.load_versioned::<Vec<u64>>(&broken).unwrap_err();
        let (key, params) = crate::error_key::parse(&err).unwrap();
        assert_eq!(key, "store.migration_failed", "{err}");
        assert_eq!(
            (params["from"].as_u64(), params["to"].as_u64()),
            (Some(1), Some(2))
        );
        assert_eq!(params["error"], "bilerek başarısız");
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1]");
        assert_eq!(files_in(dir.path()), vec!["list.json"]);

        fs::write(&path, "{not json").unwrap();
        assert!(store.load_versioned::<Vec<u64>>(&LIST_V2).is_err());
        assert!(store
            .update_versioned::<Vec<u64>, _>(&LIST_V2, |_| Ok(()))
            .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
    }

    #[test]
    fn update_versioned_serializes_writers_and_saving_over_legacy_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.json");
        fs::write(&path, "[]").unwrap();
        let store = Arc::new(JsonStore::new(&path));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || {
                    store
                        .update_versioned::<Vec<u64>, _>(&LIST_V2, |v| {
                            v.push(i);
                            Ok(())
                        })
                        .unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let mut data = store
            .load_versioned::<Vec<u64>>(&LIST_V2)
            .unwrap()
            .unwrap()
            .data;
        data.sort();
        assert_eq!(data, (0..8).collect::<Vec<_>>());
        assert_eq!(fs::read_to_string(store.backup_path(1)).unwrap(), "[]");

        // A plain save over a legacy file also keeps a backup of it.
        let other = dir.path().join("other.json");
        fs::write(&other, "[42]").unwrap();
        JsonStore::new(&other)
            .save_versioned(&LIST_V2, &vec![1u64])
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("other.json.bak-v1")).unwrap(),
            "[42]"
        );
    }

    #[test]
    fn missing_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::new(dir.path().join("none.json"));
        assert!(store
            .load_versioned::<Vec<u64>>(&LIST_V2)
            .unwrap()
            .is_none());
        let d = store
            .load_versioned_or_default::<Vec<u64>>(&LIST_V2)
            .unwrap();
        assert_eq!((d.data.len(), d.version, d.read_only), (0, 2, false));
        assert!(files_in(dir.path()).is_empty());
    }
}
