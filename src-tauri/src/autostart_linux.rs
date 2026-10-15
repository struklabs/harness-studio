//! "Start at login" on Linux: an XDG autostart entry written by Harness instead of the autostart
//! plugin, whose entry leaves the `Exec=` path unquoted (a path with a space does not start),
//! always goes to `~/.config` (ignoring `$XDG_CONFIG_HOME`) and keeps naming an AppImage after it
//! was moved or renamed.

use std::path::{Path, PathBuf};

/// Same file name the plugin used, so entries written by earlier versions are found.
const FILE_NAME: &str = "Harness Studio.desktop";

/// `$XDG_CONFIG_HOME/autostart` when that is an absolute path, else `~/.config/autostart`.
fn autostart_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart"))
}

/// Where an earlier version may have written the entry while `$XDG_CONFIG_HOME` pointed elsewhere.
fn legacy_file() -> Option<PathBuf> {
    let legacy = PathBuf::from(std::env::var_os("HOME")?)
        .join(".config/autostart")
        .join(FILE_NAME);
    (Some(&legacy) != entry_file().as_ref()).then_some(legacy)
}

fn entry_file() -> Option<PathBuf> {
    autostart_dir().map(|d| d.join(FILE_NAME))
}

/// The file the entry should start: the AppImage itself (not its temporary mount), else this
/// executable.
fn launch_target() -> Option<PathBuf> {
    match (std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR")) {
        (Some(appimage), Some(_)) => Some(PathBuf::from(appimage)),
        _ => std::env::current_exe().ok(),
    }
}

pub fn is_enabled() -> bool {
    entry_file().is_some_and(|f| f.exists())
}

pub fn enable() -> Result<(), String> {
    let file = entry_file().ok_or("no home directory")?;
    let target = launch_target().ok_or("executable path unknown")?;
    write_if_changed(&file, &entry(&target))?;
    remove_legacy();
    Ok(())
}

pub fn disable() -> Result<(), String> {
    if let Some(file) = entry_file() {
        remove(&file)?;
    }
    remove_legacy();
    Ok(())
}

/// At startup: an enabled entry is rewritten for the path this copy runs from, so moving or
/// renaming the AppImage does not leave the login entry pointing at a missing file.
pub fn refresh() {
    if !is_enabled() {
        return;
    }
    if let Err(e) = enable() {
        eprintln!("[Harness Studio] Login entry not updated: {}", e);
    }
}

fn remove_legacy() {
    if let Some(legacy) = legacy_file() {
        let _ = remove(&legacy);
    }
}

fn remove(file: &Path) -> Result<(), String> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

fn write_if_changed(file: &Path, body: &str) -> Result<(), String> {
    if std::fs::read_to_string(file).is_ok_and(|old| old == body) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(file, body).map_err(|e| e.to_string())
}

fn entry(target: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Harness Studio\n\
         Comment=Harness Studio startup\nExec={} {}\nStartupNotify=false\nTerminal=false\n",
        exec_arg(&target.to_string_lossy()),
        exec_arg(crate::desktop::MINIMIZED_ARG),
    )
}

/// One `Exec=` argument per the Desktop Entry spec: `%` doubled (field codes), an argument with a
/// reserved character in double quotes with `"`, `` ` ``, `$` and `\` escaped, then the value's
/// own escaping doubles every backslash.
fn exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = arg.replace('%', "%%");
    let quoted = if arg.contains(RESERVED) || arg.is_empty() {
        let mut out = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    } else {
        arg
    };
    quoted.replace('\\', "\\\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_arguments_follow_the_desktop_entry_spec() {
        assert_eq!(exec_arg("/opt/hs/harness-studio"), "/opt/hs/harness-studio");
        assert_eq!(
            exec_arg("/home/u/My Apps/Harness Studio.AppImage"),
            "\"/home/u/My Apps/Harness Studio.AppImage\""
        );
        assert_eq!(exec_arg("/home/u/100%/hs"), "/home/u/100%%/hs");
        // `$` and `"` escaped inside the quotes; that backslash is doubled by the value escaping.
        assert_eq!(exec_arg("/a b/$x\"y"), "\"/a b/\\\\$x\\\\\"y\"");
        assert_eq!(exec_arg("/a\\b"), "\"/a\\\\\\\\b\"");
        let body = entry(Path::new("/home/u/My Apps/hs.AppImage"));
        assert!(
            body.contains("\nExec=\"/home/u/My Apps/hs.AppImage\" --minimized\n"),
            "{body}"
        );
    }

    #[test]
    fn entry_is_rewritten_only_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("hs-autostart-test-{}", std::process::id()));
        let file = dir.join("autostart").join(FILE_NAME);
        write_if_changed(&file, "a").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a");
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_if_changed(&file, "a").unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            before
        );
        write_if_changed(&file, "b").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "b");
        remove(&file).unwrap();
        remove(&file).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
