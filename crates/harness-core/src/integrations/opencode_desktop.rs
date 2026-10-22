//! OpenCode's desktop apps: where they are, the CLI some of them ship, opening them.
//!
//! "Başlat" used to look only for the `opencode` CLI on PATH, so a machine with just the
//! desktop app got "opencode not found". The desktop app reads only the persistent config
//! (`~/.config/opencode/opencode.json`): Harness writes it (the normal preview/backup flow) and
//! then opens the app. A desktop build that ships the full CLI (`opencode-cli`) can also be
//! started in a terminal like the PATH CLI.
//!
//! Locations:
//!
//! | Platform | Location | Status |
//! |----------|----------|--------|
//! | Windows  | `%LOCALAPPDATA%\Programs\@opencode-aidesktop\OpenCode.exe` (Electron build, 1.18.x; no CLI inside) | verified 30.09 |
//! | Windows  | `%LOCALAPPDATA%\OpenCode\OpenCode.exe` + `opencode-cli.exe` (older Tauri build, 1.2.x; the CLI is the full TUI) | verified 30.09 |
//! | Windows  | Uninstall keys (HKCU/HKLM, 32-bit view too) whose `DisplayName` starts with "OpenCode": `DisplayIcon` / `InstallLocation` + `MainBinaryName` | verified 30.09 (both builds register one) |
//! | Windows  | Start menu shortcut `OpenCode.lnk` | not read: it points at the Electron exe the uninstall key already names |
//! | macOS    | `/Applications/OpenCode.app`, `~/Applications/OpenCode.app` (CLI assumed at `Contents/MacOS/opencode-cli`) | assumed |
//! | Linux    | `/opt/OpenCode/{opencode-desktop,OpenCode}`, `/usr/bin/opencode-desktop`, `~/Applications/OpenCode*.AppImage` | assumed |

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::Roots;
use crate::error_key::err;

/// How a desktop app was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopSource {
    /// A known install location.
    KnownPath,
    /// The Windows uninstall registry.
    Registry,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DesktopApp {
    /// What is opened (`OpenCode.exe`, `OpenCode.app`, an AppImage...).
    pub path: PathBuf,
    /// The CLI this build ships (`opencode-cli`), usable like the PATH `opencode`.
    pub cli: Option<PathBuf>,
    /// From the uninstall key (`DisplayVersion`, or the version in `DisplayName`).
    pub version: Option<String>,
    pub source: DesktopSource,
    /// A process with this executable is running now (it has to be restarted to read a new
    /// config).
    pub running: bool,
}

fn cli_name() -> &'static str {
    if cfg!(windows) {
        "opencode-cli.exe"
    } else {
        "opencode-cli"
    }
}

/// Known install locations for this platform (existing or not).
fn known_paths(roots: &Roots) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        if let Some(la) = &roots.localappdata {
            out.push(
                la.join("Programs")
                    .join("@opencode-aidesktop")
                    .join("OpenCode.exe"),
            );
            out.push(la.join("OpenCode").join("OpenCode.exe"));
        }
    } else if cfg!(target_os = "macos") {
        out.push(PathBuf::from("/Applications/OpenCode.app"));
        if let Some(h) = &roots.home {
            out.push(h.join("Applications").join("OpenCode.app"));
        }
    } else {
        out.push(PathBuf::from("/opt/OpenCode/opencode-desktop"));
        out.push(PathBuf::from("/opt/OpenCode/OpenCode"));
        out.push(PathBuf::from("/usr/bin/opencode-desktop"));
        if let Some(h) = &roots.home {
            let dir = h.join("Applications");
            let mut images: Vec<PathBuf> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("OpenCode") && n.ends_with(".AppImage"))
                })
                .collect();
            images.sort();
            out.extend(images);
        }
    }
    out
}

/// The CLI shipped next to (or inside) a desktop app.
fn cli_of(app: &Path) -> Option<PathBuf> {
    let candidate = if app.extension().is_some_and(|e| e == "app") {
        app.join("Contents").join("MacOS").join(cli_name())
    } else {
        app.parent()?.join(cli_name())
    };
    candidate.is_file().then_some(candidate)
}

fn exists(p: &Path) -> bool {
    // An `.app` bundle is a directory.
    p.is_file() || (p.extension().is_some_and(|e| e == "app") && p.is_dir())
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('/', "\\");
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// Entry of an uninstall key: (executable, version).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UninstallEntry {
    pub display_name: String,
    pub display_version: Option<String>,
    pub display_icon: Option<String>,
    pub install_location: Option<String>,
    pub main_binary: Option<String>,
}

impl UninstallEntry {
    /// An OpenCode entry ("OpenCode", "OpenCode 1.18.32"), not "Open Design" or others.
    pub fn is_opencode(&self) -> bool {
        let name = self.display_name.trim();
        let lower = name.to_ascii_lowercase();
        lower == "opencode" || lower.starts_with("opencode ")
    }

    /// `DisplayVersion`, else the version in the name ("OpenCode 1.18.32").
    pub fn version(&self) -> Option<String> {
        self.display_version
            .clone()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                self.display_name
                    .split_whitespace()
                    .nth(1)
                    .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .map(str::to_string)
            })
    }

    /// The app executable: `DisplayIcon` (`"C:\...\OpenCode.exe",0`) when it names an exe,
    /// else `InstallLocation` + `MainBinaryName` (default `OpenCode.exe`).
    pub fn executable(&self) -> Option<PathBuf> {
        let icon = self.display_icon.as_deref().map(|i| {
            let i = i.trim();
            // `path,index`: drop the icon index (a path never ends in ",<digits>").
            let i = match i.rsplit_once(',') {
                Some((p, idx)) if idx.trim().chars().all(|c| c.is_ascii_digit()) => p,
                _ => i,
            };
            i.trim().trim_matches('"').to_string()
        });
        if let Some(i) = icon.filter(|i| i.to_ascii_lowercase().ends_with(".exe")) {
            return Some(PathBuf::from(i));
        }
        let dir = self
            .install_location
            .as_deref()
            .map(|l| l.trim().trim_matches('"'))
            .filter(|l| !l.is_empty())?;
        let bin = self
            .main_binary
            .as_deref()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or("OpenCode.exe");
        Some(Path::new(dir).join(bin))
    }
}

/// Desktop apps found at `roots`' known locations and in `registry` (uninstall entries),
/// existing files only, without duplicates. `running` is filled by [`find`].
pub fn collect(roots: &Roots, registry: &[UninstallEntry]) -> Vec<DesktopApp> {
    let mut out: Vec<DesktopApp> = Vec::new();
    let reg: Vec<(PathBuf, Option<String>)> = registry
        .iter()
        .filter(|e| e.is_opencode())
        .filter_map(|e| Some((e.executable()?, e.version())))
        .collect();
    let version_for = |p: &Path| {
        reg.iter()
            .find(|(r, _)| same_path(r, p))
            .and_then(|(_, v)| v.clone())
    };
    for p in known_paths(roots) {
        if exists(&p) && !out.iter().any(|a| same_path(&a.path, &p)) {
            out.push(DesktopApp {
                cli: cli_of(&p),
                version: version_for(&p),
                source: DesktopSource::KnownPath,
                running: false,
                path: p,
            });
        }
    }
    for (p, version) in &reg {
        if exists(p) && !out.iter().any(|a| same_path(&a.path, p)) {
            out.push(DesktopApp {
                cli: cli_of(p),
                version: version.clone(),
                source: DesktopSource::Registry,
                running: false,
                path: p.clone(),
            });
        }
    }
    out
}

/// The OpenCode desktop apps on this machine (known locations, then the uninstall registry),
/// with whether each is running.
pub fn find(roots: &Roots) -> Vec<DesktopApp> {
    let mut apps = collect(roots, &uninstall_entries());
    if !apps.is_empty() {
        let running = running_executables();
        for a in &mut apps {
            let exe = if a.path.extension().is_some_and(|e| e == "app") {
                a.path.join("Contents").join("MacOS")
            } else {
                a.path.clone()
            };
            a.running = running
                .iter()
                .any(|r| same_path(r, &exe) || (exe.is_dir() && r.starts_with(&exe)));
        }
    }
    apps
}

/// The CLI of the first desktop app that ships one.
pub fn bundled_cli(roots: &Roots) -> Option<PathBuf> {
    collect(roots, &uninstall_entries())
        .into_iter()
        .find_map(|a| a.cli)
}

fn running_executables() -> Vec<PathBuf> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .values()
        .filter_map(|p| p.exe().map(Path::to_path_buf))
        .collect()
}

/// Opens a desktop app detached from Harness (no Harness settings in its environment: it reads
/// the persistent config). Only a path [`find`] returned is accepted. Returns the pid.
pub fn open(roots: &Roots, path: &Path, hide_external_skills: bool) -> Result<u32, String> {
    if !find(roots).iter().any(|a| same_path(&a.path, path)) {
        return Err(err("opencode_desktop.not_found")
            .with("path", path.display())
            .into());
    }
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = crate::process::hidden_command("open");
        c.arg(path);
        // LaunchServices does not inherit `open`'s environment; pass it explicitly.
        for key in super::launch::OPENCODE_SKILL_ENV {
            c.arg("--env").arg(format!(
                "{key}={}",
                if hide_external_skills { "1" } else { "0" }
            ));
        }
        c
    } else {
        // visible-console: GUI app started detached; DETACHED_PROCESS conflicts with CREATE_NO_WINDOW
        std::process::Command::new(path)
    };
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    // A config the user's shell exports would override the file Harness wrote.
    cmd.env_remove("OPENCODE_CONFIG_CONTENT");
    super::launch::apply_opencode_skill_env(&mut cmd, hide_external_skills);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = cmd.spawn().map_err(|e| {
        String::from(
            err("process.spawn_failed")
                .with("program", path.display())
                .with("error", e),
        )
    })?;
    Ok(child.id())
}

#[cfg(not(windows))]
fn uninstall_entries() -> Vec<UninstallEntry> {
    Vec::new()
}

/// Every uninstall entry of HKCU and HKLM (64- and 32-bit views).
#[cfg(windows)]
fn uninstall_entries() -> Vec<UninstallEntry> {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RRF_RT_REG_SZ,
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn value(key: HKEY, name: &str) -> Option<String> {
        let name = HSTRING::from(name);
        let mut len = 0u32;
        let r = unsafe {
            RegGetValueW(
                key,
                PCWSTR::null(),
                &name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut len),
            )
        };
        if r != ERROR_SUCCESS || len < 2 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        let r = unsafe {
            RegGetValueW(
                key,
                PCWSTR::null(),
                &name,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        buf.truncate((len as usize) / 2);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        Some(String::from_utf16_lossy(&buf))
    }

    let path = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Uninstall");
    let mut out = Vec::new();
    for (hive, view) in [
        (HKEY_CURRENT_USER, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ] {
        let mut root = HKEY::default();
        if unsafe { RegOpenKeyExW(hive, &path, None, KEY_READ | view, &mut root) } != ERROR_SUCCESS
        {
            continue;
        }
        let root = Key(root);
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(
                    root.0,
                    i,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if r != ERROR_SUCCESS {
                break;
            }
            let sub = String::from_utf16_lossy(&name[..name_len as usize]);
            let mut h = HKEY::default();
            if unsafe { RegOpenKeyExW(root.0, &HSTRING::from(sub), None, KEY_READ | view, &mut h) }
                != ERROR_SUCCESS
            {
                continue;
            }
            let key = Key(h);
            let Some(display_name) = value(key.0, "DisplayName") else {
                continue;
            };
            let entry = UninstallEntry {
                display_name,
                display_version: value(key.0, "DisplayVersion"),
                display_icon: value(key.0, "DisplayIcon"),
                install_location: value(key.0, "InstallLocation"),
                main_binary: value(key.0, "MainBinaryName"),
            };
            if entry.is_opencode() && !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use tempfile::tempdir;

    fn entry(name: &str, icon: Option<&str>, loc: Option<&str>) -> UninstallEntry {
        UninstallEntry {
            display_name: name.into(),
            display_icon: icon.map(str::to_string),
            install_location: loc.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn uninstall_entries_are_read_like_the_real_ones() {
        // The two keys found on the test machine (30.09).
        let electron = entry(
            "OpenCode 1.18.32",
            Some(r"C:\Users\u\AppData\Local\Programs\@opencode-aidesktop\OpenCode.exe,0"),
            None,
        );
        assert!(electron.is_opencode());
        assert_eq!(electron.version().as_deref(), Some("1.18.32"));
        assert_eq!(
            electron.executable(),
            Some(PathBuf::from(
                r"C:\Users\u\AppData\Local\Programs\@opencode-aidesktop\OpenCode.exe"
            ))
        );
        let tauri = UninstallEntry {
            main_binary: Some("OpenCode.exe".into()),
            ..entry(
                "OpenCode",
                Some(r#""C:\Users\u\AppData\Local\OpenCode\OpenCode.exe""#),
                Some(r#""C:\Users\u\AppData\Local\OpenCode""#),
            )
        };
        assert!(tauri.is_opencode());
        assert_eq!(tauri.version(), None);
        assert_eq!(
            tauri.executable(),
            Some(PathBuf::from(
                r"C:\Users\u\AppData\Local\OpenCode\OpenCode.exe"
            ))
        );
        let no_icon = entry("OpenCode", None, Some(r"D:\Apps\OpenCode"));
        assert_eq!(
            no_icon.executable(),
            Some(Path::new(r"D:\Apps\OpenCode").join("OpenCode.exe"))
        );
        assert!(!entry("Open Design", None, None).is_opencode());
        assert!(!entry("OpenCodeX Helper", None, None).is_opencode());
    }

    #[cfg(windows)]
    #[test]
    fn desktop_builds_are_found_with_their_cli_and_registry_version() {
        let d = tempdir().unwrap();
        let la = d.path().join("la");
        let electron = la
            .join("Programs")
            .join("@opencode-aidesktop")
            .join("OpenCode.exe");
        let tauri_dir = la.join("OpenCode");
        for f in [
            electron.clone(),
            tauri_dir.join("OpenCode.exe"),
            tauri_dir.join("opencode-cli.exe"),
        ] {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, b"").unwrap();
        }
        // Installed somewhere else: only the uninstall key knows it.
        let custom = d.path().join("apps").join("OpenCode.exe");
        std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
        std::fs::write(&custom, b"").unwrap();
        let roots = Roots {
            localappdata: Some(la.clone()),
            ..Default::default()
        };
        let reg = vec![
            entry(
                "OpenCode 1.18.32",
                Some(&format!("{},0", electron.display())),
                None,
            ),
            entry("OpenCode 1.0.0", Some(&custom.display().to_string()), None),
            entry("OpenCode 9", Some(r"C:\missing\OpenCode.exe"), None),
        ];
        let apps = collect(&roots, &reg);
        assert_eq!(apps.len(), 3, "{apps:?}");
        assert_eq!(apps[0].path, electron);
        assert_eq!(apps[0].version.as_deref(), Some("1.18.32"));
        assert_eq!(apps[0].cli, None, "the Electron build ships no CLI");
        assert_eq!(apps[1].cli, Some(tauri_dir.join("opencode-cli.exe")));
        assert_eq!(apps[2].path, custom);
        assert_eq!(apps[2].source, DesktopSource::Registry);
        assert!(open(&roots, &d.path().join("other.exe"), true).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn nothing_in_an_empty_profile() {
        let d = tempdir().unwrap();
        let roots = Roots {
            home: Some(d.path().join("home")),
            localappdata: Some(d.path().join("la")),
            ..Default::default()
        };
        assert!(collect(&roots, &[]).is_empty());
    }
}
