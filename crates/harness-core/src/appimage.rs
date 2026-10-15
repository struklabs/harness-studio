//! Programs Harness starts from inside an AppImage.
//!
//! The AppImage's AppRun points `LD_LIBRARY_PATH`, `PATH`, `XDG_DATA_DIRS`, `GTK_*`, `GST_*`,
//! `GDK_PIXBUF_MODULE_FILE`, `GIO_MODULE_DIR`... into its mount (`$APPDIR`), for the app and the
//! WebKit processes it starts from there. A program that is not part of the AppImage (a model
//! engine, Python, a terminal, an agent CLI, a shell command) would load the mount's Ubuntu
//! 22.04 libraries and GTK modules instead of its own system's, and fail or crash on newer
//! symbols. [`foreign_std`] / [`foreign_tokio`] give such a child the environment without the
//! mount: the mount's entries leave every path list (the user's own entries stay), variables
//! naming only the mount and the runtime's own (`APPDIR`, `APPIMAGE`, `ARGV0`, `OWD`) are
//! removed. Children that are this executable again (the network bridge, PDF extraction) keep
//! the environment: they need the mount's libraries. Nothing changes outside an AppImage.

// Only Linux has AppImages; elsewhere the helpers below are used by the tests alone.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Set by the AppImage runtime for the process it starts.
const RUNTIME_VARS: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// `(name, Some(value))` to set, `(name, None)` to remove; empty outside an AppImage.
pub fn foreign_env_changes() -> Vec<(OsString, Option<OsString>)> {
    #[cfg(target_os = "linux")]
    if let (Some(_), Some(appdir)) = (std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR")) {
        return changes(std::env::vars_os(), Path::new(&appdir));
    }
    Vec::new()
}

pub fn foreign_std(cmd: &mut std::process::Command) {
    for (name, value) in foreign_env_changes() {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
}

pub fn foreign_tokio(cmd: &mut tokio::process::Command) {
    for (name, value) in foreign_env_changes() {
        match value {
            Some(v) => cmd.env(name, v),
            None => cmd.env_remove(name),
        };
    }
}

fn changes(
    vars: impl Iterator<Item = (OsString, OsString)>,
    appdir: &Path,
) -> Vec<(OsString, Option<OsString>)> {
    // An empty or relative `APPDIR` would match everything.
    if !appdir.is_absolute() || appdir.parent().is_none() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, value) in vars {
        if RUNTIME_VARS.iter().any(|v| OsStr::new(v) == name) {
            out.push((name, None));
            continue;
        }
        let entries = split(&value);
        let kept: Vec<&OsStr> = entries
            .iter()
            .copied()
            .filter(|e| !Path::new(e).starts_with(appdir))
            .collect();
        if kept.len() == entries.len() {
            continue;
        }
        if kept.iter().all(|e| e.is_empty()) {
            out.push((name, None));
        } else {
            out.push((name, Some(join(&kept))));
        }
    }
    out
}

fn split(value: &OsStr) -> Vec<&OsStr> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value
            .as_bytes()
            .split(|b| *b == b':')
            .map(OsStr::from_bytes)
            .collect()
    }
    #[cfg(not(unix))]
    {
        vec![value]
    }
}

fn join(parts: &[&OsStr]) -> OsString {
    let mut out = OsString::new();
    for (i, p) in parts.iter().filter(|p| !p.is_empty()).enumerate() {
        if i > 0 {
            out.push(":");
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_entries_leave_and_user_entries_stay() {
        let m = "/tmp/.mount_hsAbc";
        let vars = [
            ("APPDIR", m.to_string()),
            ("APPIMAGE", "/home/u/Harness.AppImage".into()),
            ("OWD", "/home/u".into()),
            (
                "LD_LIBRARY_PATH",
                format!("{m}/usr/lib/:{m}/usr/lib/x86_64-linux-gnu/:/opt/cuda/lib64"),
            ),
            ("PATH", format!("{m}/usr/bin/:/usr/local/bin:/usr/bin")),
            (
                "GDK_PIXBUF_MODULE_FILE",
                format!("{m}//usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"),
            ),
            ("GST_PLUGIN_SYSTEM_PATH", format!("{m}/usr/lib/gstreamer:")),
            ("HOME", "/home/u".into()),
            // A sibling folder whose name only starts like the mount is not the mount.
            ("OTHER", format!("{m}x/bin")),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let got = changes(vars.into_iter(), Path::new(m));
        let get = |n: &str| got.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone());
        for gone in [
            "APPDIR",
            "APPIMAGE",
            "OWD",
            "GDK_PIXBUF_MODULE_FILE",
            "GST_PLUGIN_SYSTEM_PATH",
        ] {
            assert_eq!(get(gone), Some(None), "{gone}");
        }
        assert_eq!(get("LD_LIBRARY_PATH"), Some(Some("/opt/cuda/lib64".into())));
        assert_eq!(get("PATH"), Some(Some("/usr/local/bin:/usr/bin".into())));
        assert_eq!(get("HOME"), None);
        assert_eq!(get("OTHER"), None);
    }

    #[test]
    fn a_relative_or_root_appdir_changes_nothing() {
        let vars = || [(OsString::from("PATH"), OsString::from("/usr/bin"))].into_iter();
        assert!(changes(vars(), Path::new("")).is_empty());
        assert!(changes(vars(), Path::new("/")).is_empty());
        assert!(changes(vars(), Path::new("mnt")).is_empty());
    }
}
