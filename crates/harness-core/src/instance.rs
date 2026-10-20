//! Single-owner rule for the data directory (private hs-internal architecture notes).
//!
//! One process manages a data directory at a time: the GUI or a future headless
//! `harness serve`. The owner holds an OS file lock on `<data>/instance.lock` for its whole
//! life and describes itself in `<data>/instance.json` (pid, start time, kind, Control API
//! address and the path of the local session token). Other processes (the CLI) read
//! `instance.json` to find the running owner's API.
//!
//! * The lock is an OS lock (`LockFileEx` on Windows, `flock` on Unix, via `std::fs::File`):
//!   it is released by the OS when the owner exits or crashes, so a dead owner never blocks
//!   the next one. A crashed owner may leave `instance.json` behind; the next owner replaces
//!   it, and [`probe`] reports it as stale because the lock is free.
//! * A clean exit removes `instance.json` ([`InstanceLock::release`]); `instance.lock` itself
//!   stays (deleting a lock file other processes may be opening invites races).

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LOCK_FILE: &str = "instance.lock";
pub const INFO_FILE: &str = "instance.json";
const INFO_SCHEMA_VERSION: u32 = 1;

/// Which program owns the data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum InstanceKind {
    Gui,
    Headless,
}

/// Where the owner's Control API listens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiEndpoint {
    pub host: String,
    pub port: u16,
}

/// Contents of `instance.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstanceInfo {
    pub schema_version: u32,
    pub pid: u32,
    /// Unix seconds.
    pub started_at: u64,
    pub kind: InstanceKind,
    /// Version of the owning program.
    pub version: String,
    /// `None` while the Control API is off.
    pub api: Option<ApiEndpoint>,
    /// File holding the local session token (readable by the current user only).
    #[cfg_attr(feature = "openapi", schema(value_type = Option<String>))]
    pub token_path: Option<PathBuf>,
}

/// Another live process owns the data directory (`owner` = its `instance.json`, if readable).
#[derive(Debug, Clone)]
pub enum AcquireError {
    Held { owner: Option<InstanceInfo> },
    Io(String),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::error_key::err;
        let e = match self {
            AcquireError::Held { owner: Some(o) } => err("instance.held_by")
                .num("pid", o.pid)
                .with("kind", format!("{:?}", o.kind)),
            AcquireError::Held { owner: None } => err("instance.held"),
            AcquireError::Io(e) => err("instance.lock_failed").with("error", e),
        };
        write!(f, "{}", e)
    }
}

/// The held lock. Dropping it (or [`Self::release`]) removes `instance.json`; the OS lock
/// goes with the file handle.
pub struct InstanceLock {
    _file: File,
    dir: PathBuf,
    info: Mutex<InstanceInfo>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn open_lock_file(dir: &Path) -> std::io::Result<File> {
    fs::create_dir_all(dir)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE))
}

/// Reads `instance.json` (None when missing or unreadable).
pub fn read_info(dir: &Path) -> Option<InstanceInfo> {
    let text = fs::read_to_string(dir.join(INFO_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_info(dir: &Path, info: &InstanceInfo) -> Result<(), String> {
    crate::store::JsonStore::new(dir.join(INFO_FILE)).save(info)
}

impl InstanceLock {
    /// Takes ownership of `dir`. Fails with [`AcquireError::Held`] while another live process
    /// holds it; a stale `instance.json` of a crashed owner is replaced.
    pub fn acquire(dir: &Path, kind: InstanceKind, version: &str) -> Result<Self, AcquireError> {
        let file = open_lock_file(dir).map_err(|e| AcquireError::Io(e.to_string()))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                return Err(AcquireError::Held {
                    owner: read_info(dir),
                })
            }
            Err(fs::TryLockError::Error(e)) => return Err(AcquireError::Io(e.to_string())),
        }
        let info = InstanceInfo {
            schema_version: INFO_SCHEMA_VERSION,
            pid: std::process::id(),
            started_at: now_secs(),
            kind,
            version: version.to_string(),
            api: None,
            token_path: None,
        };
        write_info(dir, &info).map_err(AcquireError::Io)?;
        Ok(Self {
            _file: file,
            dir: dir.to_path_buf(),
            info: Mutex::new(info),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn info(&self) -> InstanceInfo {
        self.info.lock().unwrap().clone()
    }

    /// Records (or clears) the Control API address and token file in `instance.json`.
    pub fn set_api(
        &self,
        api: Option<ApiEndpoint>,
        token_path: Option<PathBuf>,
    ) -> Result<(), String> {
        let mut info = self.info.lock().unwrap();
        info.api = api;
        info.token_path = token_path;
        write_info(&self.dir, &info)
    }

    /// Removes `instance.json` (clean exit). The OS lock is released with the handle.
    pub fn release(&self) {
        let path = self.dir.join(INFO_FILE);
        // Only our own description: never delete a file another owner wrote meanwhile.
        if read_info(&self.dir).is_some_and(|i| i.pid == std::process::id()) {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        self.release();
    }
}

/// Who owns a data directory, as seen from another process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerState {
    /// Nobody; no leftovers.
    None,
    /// A live process holds the lock (`None` = its `instance.json` is not readable yet).
    Live(Option<InstanceInfo>),
    /// The lock is free but an `instance.json` was left behind (crashed owner).
    Stale(InstanceInfo),
}

/// Checks the owner without taking ownership (shared lock probe, released at once).
pub fn probe(dir: &Path) -> OwnerState {
    let info = read_info(dir);
    let Ok(file) = OpenOptions::new().read(true).open(dir.join(LOCK_FILE)) else {
        return match info {
            Some(i) => OwnerState::Stale(i),
            None => OwnerState::None,
        };
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let _ = file.unlock();
            match info {
                Some(i) => OwnerState::Stale(i),
                None => OwnerState::None,
            }
        }
        Err(_) => OwnerState::Live(info),
    }
}

// --- Owner-only files ---

/// Writes `data` to `path` so only the current user can read it: mode 0600 on Unix, a
/// protected DACL with a single full-access entry for the current user on Windows (applied
/// before anything is written).
pub fn write_private_file(path: &Path, data: &[u8]) -> Result<(), String> {
    use crate::error_key::err;
    use std::io::Write;
    let write_failed = |e: &dyn std::fmt::Display| -> String {
        err("fs.write_failed")
            .with("path", path.display())
            .with("error", e)
            .into()
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", parent.display())
                    .with("error", e),
            )
        })?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| write_failed(&e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| write_failed(&e))?;
    }
    #[cfg(windows)]
    win_acl::restrict_to_current_user(path).map_err(|e| write_failed(&e))?;
    file.write_all(data).map_err(|e| write_failed(&e))?;
    file.sync_all().map_err(|e| write_failed(&e))
}

/// Whether only the current user may access `path` (see [`write_private_file`]).
#[cfg(windows)]
pub(crate) fn restrict_private_file(path: &Path) -> std::io::Result<()> {
    win_acl::restrict_to_current_user(path).map_err(std::io::Error::other)
}

/// Whether only the current user may access `path` (see [`write_private_file`]).
pub fn is_private_to_current_user(path: &Path) -> Result<bool, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode();
        Ok(mode & 0o077 == 0)
    }
    #[cfg(windows)]
    {
        win_acl::only_current_user(path)
    }
}

#[cfg(windows)]
mod win_acl {
    use std::path::Path;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_SUCCESS, GENERIC_ALL, HANDLE, HLOCAL,
    };
    use windows::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
        NO_MULTIPLE_TRUSTEE, SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER,
        TRUSTEE_W,
    };
    use windows::Win32::Security::{
        AclSizeInformation, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        GetTokenInformation, TokenUser, ACCESS_ALLOWED_ACE, ACL, ACL_SIZE_INFORMATION,
        DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// The current user's SID (kept in an 8-byte aligned buffer that owns it).
    struct UserSid {
        buf: Vec<u64>,
    }

    impl UserSid {
        fn current() -> Result<Self, String> {
            // SAFETY: plain Win32 calls with buffers sized as the API reports.
            unsafe {
                let mut token = HANDLE::default();
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
                    .map_err(|e| e.to_string())?;
                let mut len = 0u32;
                let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
                let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
                let result = GetTokenInformation(
                    token,
                    TokenUser,
                    Some(buf.as_mut_ptr().cast()),
                    len,
                    &mut len,
                );
                let _ = CloseHandle(token);
                result.map_err(|e| e.to_string())?;
                Ok(Self { buf })
            }
        }

        fn psid(&self) -> PSID {
            // SAFETY: `buf` holds a TOKEN_USER written by GetTokenInformation.
            unsafe { (*(self.buf.as_ptr() as *const TOKEN_USER)).User.Sid }
        }
    }

    /// Replaces the DACL of `path` with one entry: full access for the current user; inherited
    /// entries are cut off (protected DACL).
    pub fn restrict_to_current_user(path: &Path) -> Result<(), String> {
        let user = UserSid::current()?;
        let access = EXPLICIT_ACCESS_W {
            grfAccessPermissions: GENERIC_ALL.0,
            grfAccessMode: SET_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: PWSTR(user.psid().0.cast()),
            },
        };
        let name = HSTRING::from(path.as_os_str());
        // SAFETY: `acl` is allocated by SetEntriesInAclW and freed with LocalFree.
        unsafe {
            let mut acl: *mut ACL = std::ptr::null_mut();
            let err = SetEntriesInAclW(Some(&[access]), None, &mut acl);
            if err != ERROR_SUCCESS {
                return Err(format!("SetEntriesInAclW: {:?}", err));
            }
            let err = SetNamedSecurityInfoW(
                &name,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl),
                None,
            );
            let _ = LocalFree(Some(HLOCAL(acl.cast())));
            if err != ERROR_SUCCESS {
                return Err(format!("SetNamedSecurityInfoW: {:?}", err));
            }
        }
        Ok(())
    }

    /// True when the DACL is protected and has exactly one entry, for the current user.
    pub fn only_current_user(path: &Path) -> Result<bool, String> {
        let user = UserSid::current()?;
        let name = HSTRING::from(path.as_os_str());
        // SAFETY: the descriptor is allocated by GetNamedSecurityInfoW and freed with
        // LocalFree; `dacl` points into it.
        unsafe {
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let mut sd = PSECURITY_DESCRIPTOR::default();
            let err = GetNamedSecurityInfoW(
                &name,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut sd,
            );
            if err != ERROR_SUCCESS {
                return Err(format!("GetNamedSecurityInfoW: {:?}", err));
            }
            let check = || -> Result<bool, String> {
                let mut control = 0u16;
                let mut revision = 0u32;
                GetSecurityDescriptorControl(sd, &mut control, &mut revision)
                    .map_err(|e| e.to_string())?;
                if control & SE_DACL_PROTECTED.0 == 0 || dacl.is_null() {
                    return Ok(false);
                }
                let mut size = ACL_SIZE_INFORMATION::default();
                GetAclInformation(
                    dacl,
                    (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
                .map_err(|e| e.to_string())?;
                if size.AceCount != 1 {
                    return Ok(false);
                }
                let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                GetAce(dacl, 0, &mut ace).map_err(|e| e.to_string())?;
                let ace = ace as *const ACCESS_ALLOWED_ACE;
                let sid = PSID(std::ptr::addr_of!((*ace).SidStart) as *mut _);
                Ok(EqualSid(sid, user.psid()).is_ok())
            };
            let result = check();
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_handles_contend_and_release_frees_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let first = InstanceLock::acquire(dir.path(), InstanceKind::Gui, "1.0").unwrap();
        let info = read_info(dir.path()).unwrap();
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.kind, InstanceKind::Gui);
        assert!(matches!(probe(dir.path()), OwnerState::Live(Some(_))));

        match InstanceLock::acquire(dir.path(), InstanceKind::Headless, "1.0") {
            Err(AcquireError::Held { owner: Some(o) }) => assert_eq!(o.kind, InstanceKind::Gui),
            other => panic!("expected Held, got {:?}", other.map(|_| ())),
        }

        first
            .set_api(
                Some(ApiEndpoint {
                    host: "127.0.0.1".into(),
                    port: 11500,
                }),
                Some(dir.path().join("api-token")),
            )
            .unwrap();
        assert_eq!(read_info(dir.path()).unwrap().api.unwrap().port, 11500);

        drop(first);
        assert!(
            !dir.path().join(INFO_FILE).exists(),
            "clean exit removes instance.json"
        );
        assert_eq!(probe(dir.path()), OwnerState::None);
        let second = InstanceLock::acquire(dir.path(), InstanceKind::Headless, "1.0").unwrap();
        assert_eq!(second.info().kind, InstanceKind::Headless);
    }

    #[test]
    fn stale_info_of_a_dead_owner_is_reported_and_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let stale = InstanceInfo {
            schema_version: 1,
            pid: 999_999,
            started_at: 1,
            kind: InstanceKind::Headless,
            version: "0.0.1".into(),
            api: None,
            token_path: None,
        };
        fs::write(
            dir.path().join(INFO_FILE),
            serde_json::to_string(&stale).unwrap(),
        )
        .unwrap();
        assert_eq!(probe(dir.path()), OwnerState::Stale(stale));
        let lock = InstanceLock::acquire(dir.path(), InstanceKind::Gui, "1.0").unwrap();
        assert_eq!(read_info(dir.path()).unwrap().pid, std::process::id());
        drop(lock);
    }

    /// A second process (this test binary, running [`hold_lock_child`]) holds the lock: this
    /// process cannot take it until the child exits; the OS frees it with the process.
    #[test]
    fn lock_held_by_another_process() {
        use std::io::{BufRead, BufReader, Write};
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance::tests::hold_lock_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HS_INSTANCE_LOCK_DIR", dir.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(out.read_line(&mut line).unwrap() > 0, "child exited early");
            if line.contains("HS_LOCKED") {
                break;
            }
        }
        match InstanceLock::acquire(dir.path(), InstanceKind::Gui, "1.0") {
            Err(AcquireError::Held { owner: Some(o) }) => {
                assert_eq!(o.pid, child.id());
                assert_eq!(o.kind, InstanceKind::Headless);
            }
            other => panic!("expected Held, got {:?}", other.map(|_| ())),
        }
        assert!(matches!(probe(dir.path()), OwnerState::Live(Some(_))));
        // Closing stdin lets the child exit.
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(child.wait().unwrap().success());
        let lock = InstanceLock::acquire(dir.path(), InstanceKind::Gui, "1.0").unwrap();
        assert_eq!(lock.info().pid, std::process::id());
    }

    /// Helper process for [`lock_held_by_another_process`] (does nothing when run directly).
    #[test]
    #[ignore]
    fn hold_lock_child() {
        let Ok(dir) = std::env::var("HS_INSTANCE_LOCK_DIR") else {
            return;
        };
        let lock = InstanceLock::acquire(Path::new(&dir), InstanceKind::Headless, "child").unwrap();
        println!("HS_LOCKED");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        drop(lock);
    }

    #[test]
    fn private_file_is_readable_by_the_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api-token");
        write_private_file(&path, b"secret-1").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "secret-1");
        assert!(is_private_to_current_user(&path).unwrap());
        // Rewriting keeps it private.
        write_private_file(&path, b"secret-2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "secret-2");
        assert!(is_private_to_current_user(&path).unwrap());
        // A normally created file is not.
        let open = dir.path().join("plain");
        fs::write(&open, b"x").unwrap();
        #[cfg(windows)]
        assert!(!is_private_to_current_user(&open).unwrap());
    }
}
