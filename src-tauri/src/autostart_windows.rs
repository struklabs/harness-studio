//! "Start at login" on Windows: a value under `HKCU\...\Run` written by Harness instead of the
//! autostart plugin, whose value leaves the executable path unquoted (the default install folder
//! has a space, so the command line is ambiguous) and which writes `HKLM` instead of `HKCU` when
//! the app runs elevated (an all-users entry naming one user's LocalAppData).
//!
//! Only `HKCU` is ever written. A matching `HKLM` value (exactly what the plugin wrote) is
//! removed when permitted, and reported when it keeps the entry on.

use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// Task Manager's "enabled" marker (what the plugin wrote as well).
const APPROVED_ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// The Run value: quoted executable, then the login argument.
fn run_value(exe: &str) -> String {
    format!("\"{exe}\" {}", crate::desktop::MINIMIZED_ARG)
}

/// What the plugin wrote: the executable unquoted.
fn legacy_value(exe: &str) -> String {
    format!("{exe} {}", crate::desktop::MINIMIZED_ARG)
}

/// The value to write over the existing one at startup: only the plugin's exact unquoted value
/// for this executable is rewritten; anything else (another path, a custom edit) is left alone.
fn migrated_value(current: Option<&str>, exe: &str) -> Option<String> {
    (current == Some(legacy_value(exe).as_str())).then(|| run_value(exe))
}

/// Whether the entry counts as on. An approved `HKCU` value is on; with none, a machine-wide
/// value of ours is an error (it starts the app but cannot be switched off from here).
fn status(user_value: bool, user_approved: bool, machine_ours: bool) -> Result<bool, String> {
    if user_value {
        Ok(user_approved)
    } else if machine_ours {
        Err("A machine-wide startup entry (HKLM Run) from an elevated run keeps the app starting at login. Remove it with administrator rights.".into())
    } else {
        Ok(false)
    }
}

fn exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

/// `ERROR_FILE_NOT_FOUND` as an HRESULT.
fn not_found(code: i32) -> bool {
    code as u32 == 0x8007_0002
}

fn read_run(machine: bool, name: &str) -> Result<Option<String>, String> {
    let root = if machine { LOCAL_MACHINE } else { CURRENT_USER };
    match root.open(RUN_KEY).and_then(|k| k.get_string(name)) {
        Ok(v) => Ok(Some(v)),
        Err(e) if not_found(e.code().0) => Ok(None),
        Err(e) => Err(e.message()),
    }
}

fn user_approved(name: &str) -> Result<bool, String> {
    match CURRENT_USER
        .open(APPROVED_KEY)
        .and_then(|k| k.get_value(name))
    {
        // Task Manager's "Disable" writes a timestamp into the last eight bytes.
        Ok(v) => Ok(v.len() < 8 || v.iter().rev().take(8).all(|b| *b == 0)),
        Err(e) if not_found(e.code().0) => Ok(true),
        Err(e) => Err(e.message()),
    }
}

fn machine_ours(name: &str, exe: &str) -> Result<bool, String> {
    Ok(read_run(true, name)?.is_some_and(|v| v == legacy_value(exe) || v == run_value(exe)))
}

/// Best effort: an elevated run of an earlier version may have left an all-users value.
fn remove_machine_entry(name: &str, exe: &str) {
    if machine_ours(name, exe).unwrap_or(false) {
        let _ = LOCAL_MACHINE
            .options()
            .write()
            .open(RUN_KEY)
            .and_then(|k| k.remove_value(name));
    }
}

/// `Err` when the registry cannot be read: the caller shows it instead of "off".
pub fn is_enabled(name: &str) -> Result<bool, String> {
    let exe = exe()?;
    let user = read_run(false, name)?;
    let approved = if user.is_some() {
        user_approved(name)?
    } else {
        true
    };
    status(user.is_some(), approved, machine_ours(name, &exe)?)
}

pub fn enable(name: &str) -> Result<(), String> {
    let exe = exe()?;
    CURRENT_USER
        .create(RUN_KEY)
        .and_then(|k| k.set_string(name, run_value(&exe)))
        .map_err(|e| e.message())?;
    // Turns it back on if the user had switched it off in Task Manager (the key exists only then).
    if let Ok(k) = CURRENT_USER.options().write().open(APPROVED_KEY) {
        k.set_bytes(name, windows_registry::Type::Bytes, &APPROVED_ENABLED)
            .map_err(|e| e.message())?;
    }
    remove_machine_entry(name, &exe);
    Ok(())
}

pub fn disable(name: &str) -> Result<(), String> {
    let exe = exe()?;
    match CURRENT_USER
        .options()
        .write()
        .open(RUN_KEY)
        .and_then(|k| k.remove_value(name))
    {
        Err(e) if !not_found(e.code().0) => return Err(e.message()),
        _ => {}
    }
    remove_machine_entry(name, &exe);
    Ok(())
}

/// At startup: the plugin's unquoted value is rewritten quoted.
pub fn migrate(name: &str) {
    let Ok(exe) = exe() else { return };
    let Ok(current) = read_run(false, name) else {
        return;
    };
    if let Some(value) = migrated_value(current.as_deref(), &exe) {
        let result = CURRENT_USER
            .create(RUN_KEY)
            .and_then(|k| k.set_string(name, &value));
        if let Err(e) = result {
            eprintln!("[Harness Studio] Login entry not updated: {}", e.message());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Users\Ada\AppData\Local\Harness Studio\harness-studio.exe";

    #[test]
    fn run_value_quotes_the_path_before_the_argument() {
        assert_eq!(
            run_value(EXE),
            r#""C:\Users\Ada\AppData\Local\Harness Studio\harness-studio.exe" --minimized"#
        );
        assert_eq!(legacy_value(EXE), format!("{EXE} --minimized"));
    }

    #[test]
    fn only_the_exact_unquoted_value_is_migrated() {
        assert_eq!(
            migrated_value(Some(&legacy_value(EXE)), EXE),
            Some(run_value(EXE))
        );
        assert_eq!(migrated_value(Some(&run_value(EXE)), EXE), None);
        assert_eq!(migrated_value(None, EXE), None);
        assert_eq!(
            migrated_value(Some(r"D:\other\hs.exe --minimized"), EXE),
            None
        );
        assert_eq!(migrated_value(Some(&format!("{EXE} --x")), EXE), None);
    }

    #[test]
    fn status_reports_a_machine_entry_instead_of_off() {
        assert_eq!(status(true, true, false), Ok(true));
        assert_eq!(status(true, false, false), Ok(false));
        assert_eq!(status(true, true, true), Ok(true));
        assert_eq!(status(false, true, false), Ok(false));
        assert!(status(false, true, true).is_err());
    }
}
