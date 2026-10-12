//! Where the `harness` CLI is, and "Terminalde kullan": putting it on the user's PATH.
//!
//! The desktop installer ships the CLI as a Tauri sidecar (`bundle.externalBin`), which lands in
//! the same folder as the app executable (Windows/Linux: next to `harness-studio(.exe)`, macOS:
//! `Contents/MacOS`). [`locate`] is the one function every caller uses (Harness MCP setup,
//! launch hints, diagnostics): next to the running executable first (that is the sidecar, and
//! in dev builds the CLI built into the same folder), then the first `harness` on `PATH`, then
//! a path the user typed.
//!
//! PATH changes happen only on an explicit click and are undoable:
//! * Windows: the user's `HKCU\Environment\Path` (type kept, other entries untouched byte for
//!   byte), then a `WM_SETTINGCHANGE` broadcast so new terminals see it.
//! * macOS/Linux: a symlink `~/.local/bin/harness` to the CLI; only such a symlink is removed.
//!
//! The PATH string edits are pure functions ([`path_list_add`], [`path_list_remove`]) so they
//! are unit-tested without touching the registry.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error_key::err;

/// Which lookup found the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CliSource {
    /// Next to the running executable (the bundled sidecar).
    App,
    /// The first `harness` on `PATH`.
    Path,
    /// A path the user gave.
    User,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliLocation {
    pub path: PathBuf,
    pub source: CliSource,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "harness.exe"
    } else {
        "harness"
    }
}

/// Finds the CLI: next to this executable, then `PATH`, then `user` (a file, or a folder that
/// contains it).
pub fn locate(user: Option<&str>) -> Option<CliLocation> {
    locate_in(
        std::env::current_exe().ok().as_deref(),
        super::launch::search_path().as_deref(),
        user,
    )
}

/// [`locate`] for a given app executable and `PATH` value (tests, packaging checks).
pub fn locate_in(
    current_exe: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
    user: Option<&str>,
) -> Option<CliLocation> {
    if let Some(dir) = current_exe.and_then(Path::parent) {
        let candidate = dir.join(exe_name());
        if candidate.is_file() {
            return Some(CliLocation {
                path: candidate,
                source: CliSource::App,
            });
        }
    }
    if let Some(found) = path_var.and_then(|p| {
        std::env::split_paths(p)
            .map(|d| d.join(exe_name()))
            .find(|c| c.is_file())
    }) {
        return Some(CliLocation {
            path: found,
            source: CliSource::Path,
        });
    }
    let user = user.map(str::trim).filter(|s| !s.is_empty())?;
    let p = PathBuf::from(user);
    let file = if p.is_dir() { p.join(exe_name()) } else { p };
    file.is_file().then_some(CliLocation {
        path: file,
        source: CliSource::User,
    })
}

/// `harness --version` output (`harness 0.1.0`), if the file runs.
pub fn version(cli: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(cli);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---- PATH string edits -------------------------------------------------------------------

fn norm_entry(entry: &str, ci: bool) -> String {
    let e = entry.trim().trim_matches('"');
    let e = e.trim_end_matches(['\\', '/']);
    if ci {
        e.replace('/', "\\").to_lowercase()
    } else {
        e.to_string()
    }
}

/// Whether `dir` is an entry of `list` (separator `sep`; `ci` = case-insensitive Windows rules).
pub fn path_list_contains(list: &str, dir: &str, sep: char, ci: bool) -> bool {
    let want = norm_entry(dir, ci);
    !want.is_empty() && list.split(sep).any(|e| norm_entry(e, ci) == want)
}

/// `list` with `dir` appended (unchanged when already present). A trailing separator is kept
/// trailing so [`path_list_remove`] restores the original string exactly.
pub fn path_list_add(list: &str, dir: &str, sep: char, ci: bool) -> String {
    if path_list_contains(list, dir, sep, ci) {
        return list.to_string();
    }
    if list.is_empty() {
        dir.to_string()
    } else if list.ends_with(sep) {
        format!("{list}{dir}{sep}")
    } else {
        format!("{list}{sep}{dir}")
    }
}

/// `list` without any entry equal to `dir`; every other entry stays byte for byte.
pub fn path_list_remove(list: &str, dir: &str, sep: char, ci: bool) -> String {
    let want = norm_entry(dir, ci);
    if want.is_empty() {
        return list.to_string();
    }
    list.split(sep)
        .filter(|e| norm_entry(e, ci) != want)
        .collect::<Vec<_>>()
        .join(&sep.to_string())
}

// ---- Status / install / uninstall ----------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CliStatus {
    pub path: Option<String>,
    pub source: Option<CliSource>,
    pub version: Option<String>,
    /// `windows` (user PATH entry) or `symlink` (~/.local/bin).
    pub mode: &'static str,
    /// Windows: the folder added to PATH. Unix: the symlink location.
    pub target: Option<String>,
    /// Our PATH entry / symlink exists.
    pub registered: bool,
    /// Unix: the symlink's folder is on this process's PATH (else the UI shows the hint).
    pub target_dir_on_path: bool,
}

pub fn status(cli: Option<&CliLocation>) -> CliStatus {
    CliStatus {
        path: cli.map(|c| c.path.to_string_lossy().into_owned()),
        source: cli.map(|c| c.source),
        version: cli.and_then(|c| version(&c.path)),
        mode: if cfg!(windows) { "windows" } else { "symlink" },
        registered: cli.is_some_and(|c| platform::is_registered(&c.path)),
        target_dir_on_path: platform::dir_on_process_path(),
        target: cli
            .and_then(|c| platform::target(&c.path))
            .map(|t| t.to_string_lossy().into_owned()),
    }
}

/// Adds the CLI to the user's PATH (Windows) or links it into `~/.local/bin` (elsewhere).
pub fn install(cli: &Path) -> Result<(), String> {
    if !cli.is_file() {
        return Err(err("fs.not_found").with("path", cli.display()).into());
    }
    platform::install(cli)
}

/// Undoes [`install`]; a PATH entry / symlink that is not ours is left alone.
pub fn uninstall(cli: &Path) -> Result<(), String> {
    platform::uninstall(cli)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(access: REG_SAM_FLAGS) -> Result<Key, String> {
        let mut h = HKEY::default();
        let r =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, w!("Environment"), None, access, &mut h) };
        if r == ERROR_SUCCESS {
            Ok(Key(h))
        } else {
            Err(err("cli_install.registry_open_failed")
                .num("code", r.0)
                .into())
        }
    }

    /// (value, type); `None` when the user has no `Path` value.
    fn read_user_path() -> Result<Option<(String, REG_VALUE_TYPE)>, String> {
        let key = open(KEY_READ)?;
        let mut ty = REG_VALUE_TYPE(0);
        let mut len = 0u32;
        let name = w!("Path");
        let r = unsafe { RegQueryValueExW(key.0, name, None, Some(&mut ty), None, Some(&mut len)) };
        if r == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if r != ERROR_SUCCESS {
            return Err(err("cli_install.registry_read_failed")
                .num("code", r.0)
                .into());
        }
        let mut buf = vec![0u8; len as usize];
        let r = unsafe {
            RegQueryValueExW(
                key.0,
                name,
                None,
                Some(&mut ty),
                Some(buf.as_mut_ptr()),
                Some(&mut len),
            )
        };
        if r != ERROR_SUCCESS {
            return Err(err("cli_install.registry_read_failed")
                .num("code", r.0)
                .into());
        }
        buf.truncate(len as usize);
        let mut wide: Vec<u16> = buf
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        while wide.last() == Some(&0) {
            wide.pop();
        }
        Ok(Some((String::from_utf16_lossy(&wide), ty)))
    }

    fn write_user_path(value: &str, ty: REG_VALUE_TYPE) -> Result<(), String> {
        let key = open(KEY_SET_VALUE)?;
        let mut bytes: Vec<u8> = Vec::with_capacity(value.len() * 2 + 2);
        for u in value.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let r = unsafe { RegSetValueExW(key.0, w!("Path"), None, ty, Some(&bytes)) };
        if r != ERROR_SUCCESS {
            return Err(err("cli_install.registry_write_failed")
                .num("code", r.0)
                .into());
        }
        broadcast();
        Ok(())
    }

    /// Tells running programs (Explorer, so new terminals) that the environment changed.
    fn broadcast() {
        let env: Vec<u16> = "Environment\0".encode_utf16().collect();
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(PCWSTR(env.as_ptr()).0 as isize),
                SMTO_ABORTIFHUNG,
                5000,
                Some(&mut result),
            );
        }
    }

    fn dir_of(cli: &Path) -> Option<String> {
        cli.parent().map(|p| p.to_string_lossy().into_owned())
    }

    pub(super) fn target(cli: &Path) -> Option<PathBuf> {
        cli.parent().map(Path::to_path_buf)
    }

    pub(super) fn is_registered(cli: &Path) -> bool {
        let (Some(dir), Ok(Some((val, _)))) = (dir_of(cli), read_user_path()) else {
            return false;
        };
        path_list_contains(&val, &dir, ';', true)
    }

    pub(super) fn dir_on_process_path() -> bool {
        true
    }

    pub(super) fn install(cli: &Path) -> Result<(), String> {
        let dir = dir_of(cli)
            .ok_or_else(|| err("cli_install.no_parent_dir").with("path", cli.display()))?;
        let (val, ty) = read_user_path()?.unwrap_or((String::new(), REG_EXPAND_SZ));
        let ty = if ty == REG_SZ { REG_SZ } else { REG_EXPAND_SZ };
        let next = path_list_add(&val, &dir, ';', true);
        if next == val {
            return Ok(());
        }
        write_user_path(&next, ty)
    }

    pub(super) fn uninstall(cli: &Path) -> Result<(), String> {
        let dir = dir_of(cli)
            .ok_or_else(|| err("cli_install.no_parent_dir").with("path", cli.display()))?;
        let Some((val, ty)) = read_user_path()? else {
            return Ok(());
        };
        let next = path_list_remove(&val, &dir, ';', true);
        if next == val {
            return Ok(());
        }
        write_user_path(&next, ty)
    }

    /// For tests: raw read/write of the user PATH (value, is_expand).
    #[cfg(test)]
    pub(super) fn raw_get() -> Option<(String, bool)> {
        read_user_path()
            .unwrap()
            .map(|(v, t)| (v, t == REG_EXPAND_SZ))
    }
    #[cfg(test)]
    pub(super) fn raw_set(v: &str, expand: bool) {
        write_user_path(v, if expand { REG_EXPAND_SZ } else { REG_SZ }).unwrap()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    fn link_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        Some(home.join(".local").join("bin").join(exe_name()))
    }

    pub(super) fn target(_cli: &Path) -> Option<PathBuf> {
        link_path()
    }

    fn ours(link: &Path, cli: &Path) -> bool {
        match std::fs::read_link(link) {
            Ok(t) => t == cli || t.file_name().is_some_and(|n| n == exe_name()),
            Err(_) => false,
        }
    }

    pub(super) fn is_registered(cli: &Path) -> bool {
        link_path().is_some_and(|l| ours(&l, cli))
    }

    pub(super) fn dir_on_process_path() -> bool {
        let (Some(link), Some(path)) = (link_path(), std::env::var_os("PATH")) else {
            return false;
        };
        let dir = link.parent().map(Path::to_path_buf);
        std::env::split_paths(&path).any(|d| Some(d) == dir)
    }

    pub(super) fn install(cli: &Path) -> Result<(), String> {
        let link = link_path().ok_or_else(|| err("integrations.home_not_found"))?;
        if let Some(dir) = link.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e)
            })?;
        }
        if link.symlink_metadata().is_ok() {
            if ours(&link, cli) {
                std::fs::remove_file(&link).map_err(|e| {
                    err("fs.remove_failed")
                        .with("path", link.display())
                        .with("error", e)
                })?;
            } else {
                return Err(err("cli_install.link_exists")
                    .with("path", link.display())
                    .into());
            }
        }
        std::os::unix::fs::symlink(cli, &link).map_err(|e| {
            err("cli_install.link_failed")
                .with("path", link.display())
                .with("error", e)
                .into()
        })
    }

    pub(super) fn uninstall(cli: &Path) -> Result<(), String> {
        let Some(link) = link_path() else {
            return Ok(());
        };
        if ours(&link, cli) {
            std::fs::remove_file(&link).map_err(|e| {
                err("fs.remove_failed")
                    .with("path", link.display())
                    .with("error", e)
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_round_trip_is_exact() {
        for original in [
            r"C:\a;C:\b",
            r"C:\a;C:\b;",
            "",
            r"%USERPROFILE%\bin;;C:\x y\z",
            r"C:\a",
        ] {
            let added = path_list_add(original, r"C:\App", ';', true);
            assert!(path_list_contains(&added, r"c:\app\", ';', true));
            assert_eq!(path_list_remove(&added, r"C:\App", ';', true), original);
        }
    }

    #[test]
    fn add_is_idempotent_and_case_insensitive_on_windows() {
        let once = path_list_add(r"C:\a", r"C:\App", ';', true);
        assert_eq!(path_list_add(&once, r"c:\APP\", ';', true), once);
        // Unix paths are case-sensitive.
        assert_ne!(path_list_add("/a:/App", "/app", ':', false), "/a:/App");
    }

    #[test]
    fn remove_leaves_others_and_handles_quotes() {
        assert_eq!(
            path_list_remove(r#"C:\a;"C:\App";C:\b"#, r"C:\App", ';', true),
            r"C:\a;C:\b"
        );
        assert_eq!(path_list_remove(r"C:\a", r"C:\App", ';', true), r"C:\a");
        assert_eq!(path_list_remove(r"C:\a", "", ';', true), r"C:\a");
    }

    #[test]
    fn locate_order_app_then_path_then_user() {
        let app = tempfile::tempdir().unwrap();
        let onpath = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        for d in [&app, &onpath, &user] {
            std::fs::write(d.path().join(exe_name()), b"x").unwrap();
        }
        let exe = app.path().join("harness-studio.exe");
        let pathvar = std::env::join_paths([onpath.path()]).unwrap();
        let u = user.path().to_string_lossy().to_string();

        let l = locate_in(Some(&exe), Some(&pathvar), Some(&u)).unwrap();
        assert_eq!(l.source, CliSource::App);

        std::fs::remove_file(app.path().join(exe_name())).unwrap();
        let l = locate_in(Some(&exe), Some(&pathvar), Some(&u)).unwrap();
        assert_eq!(
            (l.source, l.path),
            (CliSource::Path, onpath.path().join(exe_name()))
        );

        std::fs::remove_file(onpath.path().join(exe_name())).unwrap();
        let l = locate_in(Some(&exe), Some(&pathvar), Some(&u)).unwrap();
        assert_eq!(l.source, CliSource::User);
        assert!(locate_in(Some(&exe), Some(&pathvar), None).is_none());
    }

    /// Real registry round trip on the current user's PATH. Opt-in (`HS_TEST_USER_PATH=1`)
    /// because it writes HKCU; the original value and type are restored and compared.
    #[cfg(windows)]
    #[test]
    fn user_path_registry_round_trip() {
        if std::env::var_os("HS_TEST_USER_PATH").is_none() {
            return;
        }
        let before = platform::raw_get();
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("harness.exe");
        std::fs::write(&cli, b"x").unwrap();
        let result = std::panic::catch_unwind(|| {
            install(&cli).unwrap();
            assert!(platform::is_registered(&cli));
            uninstall(&cli).unwrap();
            assert!(!platform::is_registered(&cli));
        });
        if let Some((v, expand)) = &before {
            if platform::raw_get().as_ref() != before.as_ref() {
                platform::raw_set(v, *expand);
            }
        }
        assert_eq!(platform::raw_get(), before, "user PATH must be unchanged");
        result.unwrap();
    }
}
