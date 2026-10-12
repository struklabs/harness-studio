//! Shared GPU libraries: one copy of a runtime's companion archive (the CUDA runtime zip
//! `cudart-llama-bin-<os>-cuda-<v>-<arch>.zip`: cudart, cuBLAS, cuBLASLt, ~550 MB unpacked)
//! for every runtime that uses it, instead of one download and one copy per build.
//!
//! * Store: `bin/shared/<sha256 of the archive>/` holds the archive's files. The key is the
//!   published digest the download was verified against, so a stored archive is the same
//!   bytes every release that names that digest would download.
//! * Runtime folders get hard links to the stored files (the loader finds them beside
//!   `llama-server` as before). Where a hard link cannot be made (another volume, a file
//!   system without links) the file is copied.
//! * Reference count = the file system's link count: a stored file whose only link is the
//!   store itself belongs to no runtime any more. [`SharedStore::prune`] removes such
//!   archives; it runs after every install, activation, rollback and removal. A crash between
//!   two steps leaves at most an unreferenced archive, which the next prune removes.
//!
//! Windows: a link of a DLL a running server has loaded can be deleted (checked on Windows
//! 11 with llama-server b11327 holding cuBLAS), so removing one runtime never waits for a
//! server of another.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File name prefix of the companion archives that are shared.
const COMPANION_PREFIX: &str = "cudart-";

/// Whether an asset is a shareable companion archive (not a build of llama.cpp itself).
pub fn is_companion(asset_name: &str) -> bool {
    asset_name
        .to_ascii_lowercase()
        .starts_with(COMPANION_PREFIX)
}

#[derive(Debug, Clone)]
pub struct SharedStore {
    root: PathBuf,
}

impl SharedStore {
    /// The store beside the runtime folders (`<bin>/shared`).
    pub fn in_bin(bin: &Path) -> Self {
        Self {
            root: bin.join("shared"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Folder of one archive; `None` for anything but a lower-case SHA-256 hex digest.
    fn archive_dir(&self, digest: &str) -> Option<PathBuf> {
        let ok = digest.len() == 64
            && digest
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        ok.then(|| self.root.join(digest))
    }

    /// Exclusive lock of the store across processes (the app, `harness`, a second install):
    /// every operation below runs under it, so a prune never sees another install's archive
    /// between its unpack and its first link. Released when the file is dropped.
    fn lock(&self) -> io::Result<fs::File> {
        fs::create_dir_all(&self.root)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join(".lock"))?;
        file.lock()?;
        Ok(file)
    }

    /// Links the stored files of archive `digest` into `dest`. `true` when the archive is in
    /// the store and every file was placed (the download can be skipped).
    pub fn restore(&self, digest: &str, dest: &Path) -> bool {
        let Some(dir) = self.archive_dir(digest) else {
            return false;
        };
        let Ok(_lock) = self.lock() else {
            return false;
        };
        let Ok(files) = stored_files(&dir) else {
            return false;
        };
        if files.is_empty() {
            return false;
        }
        files
            .iter()
            .all(|name| link_or_copy(&dir.join(name), &dest.join(name)).is_ok())
    }

    /// Unpacks a downloaded, verified companion archive into the store and links its files
    /// into `dest`. On error nothing is left in the store for this digest; the caller falls
    /// back to unpacking into `dest` directly.
    pub fn add(&self, digest: &str, archive: &Path, dest: &Path) -> Result<(), String> {
        let dir = self
            .archive_dir(digest)
            .ok_or_else(|| format!("bad digest: {digest}"))?;
        let _lock = self.lock().map_err(|e| e.to_string())?;
        let part = self.root.join(format!("{digest}.part"));
        let _ = fs::remove_dir_all(&part);
        fs::create_dir_all(&part).map_err(|e| e.to_string())?;
        let placed = (|| {
            crate::runtime::extract_archive(archive, &part)?;
            if dir.exists() {
                // Left by an interrupted run: the new copy replaces it.
                fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            }
            fs::rename(&part, &dir).map_err(|e| e.to_string())?;
            for name in stored_files(&dir).map_err(|e| e.to_string())? {
                link_or_copy(&dir.join(&name), &dest.join(&name)).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        if placed.is_err() {
            let _ = fs::remove_dir_all(&part);
            if !self.is_referenced(&dir) {
                let _ = fs::remove_dir_all(&dir);
            }
        }
        placed
    }

    /// Whether any stored file of the archive folder still has a link outside the store.
    fn is_referenced(&self, dir: &Path) -> bool {
        stored_files(dir).is_ok_and(|files| {
            files
                .iter()
                .any(|f| link_count(&dir.join(f)).is_none_or(|n| n > 1))
        })
    }

    /// Removes archives no runtime links to any more and leftovers of interrupted unpacks.
    /// Returns the bytes freed. An archive whose link count cannot be read is kept.
    pub fn prune(&self) -> u64 {
        if !self.root.exists() {
            return 0;
        }
        let Ok(_lock) = self.lock() else {
            return 0;
        };
        let Ok(entries) = fs::read_dir(&self.root) else {
            return 0;
        };
        let mut freed = 0;
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue; // the lock file
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // Under the lock no unpack is in progress: a `.part` folder is a crash leftover.
            let leftover = name.ends_with(".part");
            if !leftover && (self.archive_dir(&name).is_none() || self.is_referenced(&path)) {
                continue;
            }
            let size = dir_size(&path);
            if fs::remove_dir_all(&path).is_ok() {
                freed += size;
            }
        }
        freed
    }

    /// Turns a runtime folder's own copy of archive `digest` (installed before the store
    /// existed) into links to the store. When the store lacks the archive, the folder's
    /// companion files seed it (links, nothing copied). Otherwise every stored file must be
    /// in the folder with the same SHA-256, or the folder is left as it is. Each file is
    /// swapped through a temporary link, so a failure never leaves a name missing; a running
    /// server keeps the data it loaded. Returns the bytes freed.
    pub fn adopt(&self, digest: &str, dir: &Path) -> Result<u64, String> {
        let archive = self
            .archive_dir(digest)
            .ok_or_else(|| format!("bad digest: {digest}"))?;
        let _lock = self.lock().map_err(|e| e.to_string())?;
        if !archive.exists() {
            let names: Vec<String> = stored_files(dir)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|n| is_companion_file(n))
                .collect();
            if names.is_empty() {
                return Ok(0);
            }
            let part = self.root.join(format!("{digest}.part"));
            let _ = fs::remove_dir_all(&part);
            fs::create_dir_all(&part).map_err(|e| e.to_string())?;
            let seeded = names
                .iter()
                .try_for_each(|n| fs::hard_link(dir.join(n), part.join(n)))
                .and_then(|_| fs::rename(&part, &archive));
            if let Err(e) = seeded {
                let _ = fs::remove_dir_all(&part);
                return Err(e.to_string());
            }
            return Ok(0);
        }

        let stored = stored_files(&archive).map_err(|e| e.to_string())?;
        let mut swap = Vec::new();
        for name in &stored {
            let (ours, theirs) = (dir.join(name), archive.join(name));
            let (Some(a), Some(b)) = (file_info(&ours), file_info(&theirs)) else {
                return Ok(0); // missing in the folder: not this archive's copy
            };
            if a.id == b.id {
                continue; // already shared
            }
            let same = fs::metadata(&ours).map(|m| m.len()).ok()
                == fs::metadata(&theirs).map(|m| m.len()).ok()
                && crate::runtimes::sha256_file(&ours)? == crate::runtimes::sha256_file(&theirs)?;
            if !same {
                return Ok(0);
            }
            swap.push((name, a.links));
        }
        let mut freed = 0;
        for (name, links) in swap {
            let (ours, theirs) = (dir.join(name), archive.join(name));
            let tmp = dir.join(format!(".{name}.shared"));
            let _ = fs::remove_file(&tmp);
            if fs::hard_link(&theirs, &tmp).is_err() {
                continue; // another volume: keep the copy
            }
            let size = fs::metadata(&ours).map(|m| m.len()).unwrap_or(0);
            if let Err(e) = fs::rename(&tmp, &ours) {
                let _ = fs::remove_file(&tmp);
                return Err(e.to_string());
            }
            if links == 1 {
                freed += size;
            }
        }
        Ok(freed)
    }
}

/// One pass over the installed builds (once per app start): their own copies of a CUDA
/// runtime archive become links to the store. GitHub runtimes name the archive and its digest
/// in their record and must be unchanged since install; the managed build and its backup get
/// the digest from their release (one GitHub request per tag, skipped offline). Returns the
/// bytes freed.
pub async fn dedupe_installed(
    runtime: &crate::runtime::RuntimeManager,
    registry: &crate::runtimes::RuntimeRegistry,
) -> u64 {
    use crate::runtimes::{RuntimeSource, OFFICIAL_ID};
    let mut work: Vec<(PathBuf, String, Option<String>)> = Vec::new();
    for e in registry.entries().unwrap_or_default() {
        let RuntimeSource::Github { assets, .. } = &e.source else {
            continue;
        };
        if e.id == OFFICIAL_ID
            || !registry.owns_dir(&e)
            || !crate::runtimes::changed_files(&e).is_empty()
        {
            continue;
        }
        for a in assets.iter().filter(|a| is_companion(&a.name)) {
            if let Some(d) = &a.sha256 {
                work.push((e.dir.clone(), d.clone(), Some(e.id.clone())));
            }
        }
    }
    for dir in [runtime.managed_dir(), runtime.backup_dir()] {
        let manifest = crate::runtime::read_manifest(&dir);
        if let Some(d) = manifest.companion_sha256 {
            work.push((dir, d, None));
            continue;
        }
        // A build from before the digest was recorded: ask its release once, keep the answer.
        let (Some(tag), Some("cuda")) = (manifest.version, manifest.backend.as_deref()) else {
            continue;
        };
        let Some(major) = cuda_major(&dir) else {
            continue;
        };
        if let Some(d) = runtime.companion_digest(&tag, &major).await {
            let _ = crate::runtime::record_companion(&dir, &d);
            work.push((dir, d, None));
        }
    }

    let store = registry.shared_store();
    let mut freed = 0;
    for (dir, digest, entry_id) in work {
        let (s, d) = (store.clone(), dir.clone());
        let reg = registry.clone();
        let result = tokio::task::spawn_blocking(move || {
            let freed = s.adopt(&digest, &d)?;
            // The links carry the store file's times: re-record them so the launch check
            // does not see a changed runtime.
            if let Some(id) = entry_id {
                let mut e = reg.get(&id)?;
                for f in e.files.iter_mut() {
                    if let Ok(m) = fs::metadata(d.join(&f.name)) {
                        f.mtime_ms = crate::runtimes::mtime_ms(&m);
                    }
                }
                reg.save_entry(e)?;
            }
            Ok::<u64, String>(freed)
        })
        .await;
        // A folder that cannot be shared keeps its own copy; nothing else depends on it.
        if let Ok(Ok(n)) = result {
            freed += n;
        }
    }
    freed
}

/// `12` from `cudart64_12.dll` in a runtime folder.
fn cuda_major(dir: &Path) -> Option<String> {
    stored_files(dir).ok()?.into_iter().find_map(|n| {
        let lower = n.to_ascii_lowercase();
        let rest = lower.strip_prefix("cudart64_")?.strip_suffix(".dll")?;
        rest.chars()
            .all(|c| c.is_ascii_digit())
            .then(|| rest.to_string())
    })
}

/// Files of the CUDA runtime archive (cudart, cuBLAS, cuBLASLt) in a runtime folder.
fn is_companion_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["cudart64_", "cublas64_", "cublaslt64_"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// Regular files directly in an archive folder (companion archives are flat).
fn stored_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out.sort();
    Ok(out)
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Hard link `src` at `dst` (replacing a file there), else a copy.
fn link_or_copy(src: &Path, dst: &Path) -> io::Result<()> {
    if dst.exists() {
        fs::remove_file(dst)?;
    }
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    fs::copy(src, dst).map(|_| ())
}

/// Number of names (hard links) of a file.
pub fn link_count(path: &Path) -> Option<u64> {
    file_info(path).map(|i| i.links)
}

/// Identity (device, file number) and link count of a file.
struct FileInfo {
    id: (u64, u64),
    links: u64,
}

#[cfg(unix)]
fn file_info(path: &Path) -> Option<FileInfo> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path).ok()?;
    Some(FileInfo {
        id: (m.dev(), m.ino()),
        links: m.nlink(),
    })
}

#[cfg(windows)]
fn file_info(path: &Path) -> Option<FileInfo> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let file = fs::File::open(path).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the duration of the call; `info` is a valid out pointer.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }.ok()?;
    Some(FileInfo {
        id: (
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ),
        links: u64::from(info.nNumberOfLinks),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const DIGEST: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn companions_are_the_cuda_runtime_archives() {
        assert!(is_companion("cudart-llama-bin-win-cuda-12.4-x64.zip"));
        assert!(!is_companion("llama-b11327-bin-win-cuda-12.4-x64.zip"));
    }

    #[test]
    fn two_runtimes_share_one_copy_until_the_last_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path();
        let store = SharedStore::in_bin(bin);
        let archive = bin.join("cudart.zip");
        zip(
            &archive,
            &[
                ("cublas64_12.dll", b"cublas"),
                ("cudart64_12.dll", b"cudart"),
            ],
        );
        let (a, b) = (bin.join("managed"), bin.join("runtimes").join("fork"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();

        // First runtime downloads; the second finds the archive and needs no download.
        assert!(!store.restore(DIGEST, &b));
        store.add(DIGEST, &archive, &a).unwrap();
        assert!(store.restore(DIGEST, &b));
        for dir in [&a, &b] {
            assert_eq!(fs::read(dir.join("cublas64_12.dll")).unwrap(), b"cublas");
        }
        let stored = store.root().join(DIGEST).join("cublas64_12.dll");
        assert_eq!(link_count(&stored), Some(3));

        // One runtime goes: the other keeps working, the store keeps the archive.
        fs::remove_dir_all(&a).unwrap();
        assert_eq!(store.prune(), 0);
        assert_eq!(fs::read(b.join("cudart64_12.dll")).unwrap(), b"cudart");
        assert_eq!(link_count(&stored), Some(2));

        // The last one goes: the archive is freed.
        fs::remove_dir_all(&b).unwrap();
        assert_eq!(store.prune(), 12);
        assert!(!store.root().join(DIGEST).exists());
        assert!(!store.restore(DIGEST, &a));
    }

    /// Two installs and a prune at the same time (the UI can start two installs): without the
    /// lock the prune deleted an archive between its unpack and its first link, or the second
    /// unpack's `.part`.
    #[test]
    fn concurrent_installs_and_prunes_keep_the_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().to_path_buf();
        let archive = bin.join("cudart.zip");
        zip(&archive, &[("cublas64_12.dll", &[7u8; 4096])]);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pruner = {
            let (store, stop) = (SharedStore::in_bin(&bin), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    store.prune();
                }
            })
        };
        let installers: Vec<_> = (0..2)
            .map(|i| {
                let (bin, archive) = (bin.clone(), archive.clone());
                std::thread::spawn(move || {
                    let store = SharedStore::in_bin(&bin);
                    for round in 0..40 {
                        let dest = bin.join(format!("rt-{i}-{round}"));
                        fs::create_dir_all(&dest).unwrap();
                        if !store.restore(DIGEST, &dest) {
                            store.add(DIGEST, &archive, &dest).unwrap();
                        }
                        assert_eq!(fs::read(dest.join("cublas64_12.dll")).unwrap().len(), 4096);
                        fs::remove_dir_all(&dest).unwrap();
                    }
                })
            })
            .collect();
        for t in installers {
            t.join().unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        pruner.join().unwrap();
        SharedStore::in_bin(&bin).prune();
        assert!(!bin.join("shared").join(DIGEST).exists());
    }

    #[test]
    fn installed_copies_are_adopted_byte_for_byte() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SharedStore::in_bin(tmp.path());
        let rt = |name: &str, cublas: &[u8]| {
            let d = tmp.path().join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("cublas64_12.dll"), cublas).unwrap();
            fs::write(d.join("cudart64_12.dll"), b"rt").unwrap();
            fs::write(d.join("ggml-cuda.dll"), name.as_bytes()).unwrap();
            d
        };
        let (managed, backup, other) = (
            rt("managed", b"cublas"),
            rt("backup", b"cublas"),
            rt("other", b"CUBLAS"),
        );
        assert_eq!(cuda_major(&managed).as_deref(), Some("12"));

        // The first folder seeds the store with links (nothing freed, nothing copied).
        assert_eq!(store.adopt(DIGEST, &managed).unwrap(), 0);
        let stored = store.root().join(DIGEST);
        assert_eq!(
            stored_files(&stored).unwrap(),
            ["cublas64_12.dll", "cudart64_12.dll"]
        );
        assert_eq!(link_count(&managed.join("cublas64_12.dll")), Some(2));
        // An identical copy becomes a link; the build's own library stays its own.
        assert_eq!(store.adopt(DIGEST, &backup).unwrap(), 8);
        assert_eq!(link_count(&stored.join("cublas64_12.dll")), Some(3));
        assert_eq!(fs::read(backup.join("cublas64_12.dll")).unwrap(), b"cublas");
        assert_eq!(link_count(&backup.join("ggml-cuda.dll")), Some(1));
        assert!(!backup.join(".cublas64_12.dll.shared").exists());
        // Again: already shared, nothing to do.
        assert_eq!(store.adopt(DIGEST, &backup).unwrap(), 0);
        // Different bytes under the same names: left alone.
        assert_eq!(store.adopt(DIGEST, &other).unwrap(), 0);
        assert_eq!(fs::read(other.join("cublas64_12.dll")).unwrap(), b"CUBLAS");
        assert_eq!(link_count(&other.join("cublas64_12.dll")), Some(1));
    }

    /// A build whose manifest carries the archive digest is shared without asking GitHub,
    /// on every start (offline included).
    #[tokio::test]
    async fn recorded_digest_needs_no_github_request() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        let mut runtime = crate::runtime::RuntimeManager::default();
        runtime.base_bin_dir = Some(bin.clone());
        let registry = crate::runtimes::RuntimeRegistry::with_paths(
            tmp.path().join("runtimes.json"),
            bin.join("runtimes"),
        );
        for dir in [runtime.managed_dir(), runtime.backup_dir()] {
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("cublas64_12.dll"), b"cublas").unwrap();
            fs::write(dir.join("cudart64_12.dll"), b"rt").unwrap();
            fs::write(dir.join("ggml-cuda.dll"), b"x").unwrap();
            fs::write(
                dir.join(crate::runtime::RUNTIME_MANIFEST),
                r#"{"version":"b11333","backend":"cuda"}"#,
            )
            .unwrap();
            crate::runtime::record_companion(&dir, DIGEST).unwrap();
            let m = crate::runtime::read_manifest(&dir);
            assert_eq!(m.version.as_deref(), Some("b11333"));
            assert_eq!(m.companion_sha256.as_deref(), Some(DIGEST));
        }
        assert_eq!(dedupe_installed(&runtime, &registry).await, 8);
        assert_eq!(dedupe_installed(&runtime, &registry).await, 0);
        assert_eq!(runtime.companion_lookups(), 0);
        assert_eq!(
            link_count(&runtime.backup_dir().join("cublas64_12.dll")),
            Some(3)
        );
    }

    #[test]
    fn bad_digests_and_leftovers() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SharedStore::in_bin(tmp.path());
        assert!(!store.restore("../x", tmp.path()));
        assert!(store
            .add("../x", &tmp.path().join("a.zip"), tmp.path())
            .is_err());
        // A broken archive leaves nothing behind.
        let bad = tmp.path().join("bad.zip");
        fs::write(&bad, b"not a zip").unwrap();
        assert!(store.add(DIGEST, &bad, tmp.path()).is_err());
        assert!(!store.root().join(DIGEST).exists());
        // An interrupted unpack is cleaned up.
        fs::create_dir_all(store.root().join(format!("{DIGEST}.part"))).unwrap();
        fs::write(store.root().join(format!("{DIGEST}.part")).join("x"), b"ab").unwrap();
        assert_eq!(store.prune(), 2);
    }
}

/// The real pass over this machine's installed builds (the app's data folder).
/// `HS_DATA_DIR=%APPDATA%/com.harnessstudio.app cargo test -p harness-core --lib real_dedupe_installed -- --ignored --nocapture`
#[cfg(test)]
#[tokio::test]
#[ignore]
async fn real_dedupe_installed() {
    let Ok(root) = std::env::var("HS_DATA_DIR") else {
        eprintln!("HS_DATA_DIR not set, skipping");
        return;
    };
    crate::paths::init(root.into());
    let runtime = crate::runtime::RuntimeManager::default();
    let registry = crate::runtimes::RuntimeRegistry::global();
    let freed = dedupe_installed(&runtime, &registry).await;
    eprintln!("freed {} MB", freed >> 20);
    for dir in [runtime.managed_dir(), runtime.backup_dir()] {
        for name in stored_files(&dir).unwrap_or_default() {
            if is_companion_file(&name) {
                eprintln!(
                    "{}: {:?} links",
                    dir.join(&name).display(),
                    link_count(&dir.join(&name))
                );
            }
        }
    }
}
