use chrono::{Datelike, Local, SecondsFormat, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};

use super::simple_def;
use crate::tools::{PermissionClass, ToolDef, ToolError, ToolOutput};

pub fn def() -> ToolDef {
    simple_def(
        "current_datetime",
        "Get the current date and time in UTC and local time, including ISO 8601 values, timezone, offset, and weekday. Optionally request an IANA timezone such as Europe/Istanbul.",
        PermissionClass::Safe,
        json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "Optional IANA timezone name, for example Europe/Istanbul or America/New_York."
                }
            },
            "additionalProperties": false
        }),
    )
}

pub fn run(args: Value) -> Result<ToolOutput, ToolError> {
    let utc = Utc::now();
    let requested = args.get("timezone").and_then(Value::as_str);
    let (local_iso, timezone, offset, weekday) = if let Some(name) = requested {
        let tz: Tz = name.parse().map_err(|_| {
            ToolError::invalid(format!(
                "unknown IANA timezone {name:?}; use a name such as Europe/Istanbul"
            ))
        })?;
        let dt = utc.with_timezone(&tz);
        (
            dt.to_rfc3339_opts(SecondsFormat::Secs, true),
            name.to_string(),
            dt.format("%:z").to_string(),
            dt.weekday().to_string(),
        )
    } else {
        let dt = utc.with_timezone(&Local);
        (
            dt.to_rfc3339_opts(SecondsFormat::Secs, true),
            dt.format("%Z").to_string(),
            dt.format("%:z").to_string(),
            dt.weekday().to_string(),
        )
    };
    Ok(ToolOutput::text(
        json!({
            "local": local_iso,
            "utc": utc.to_rfc3339_opts(SecondsFormat::Secs, true),
            "timezone": timezone,
            "utc_offset": offset,
            "weekday": weekday
        })
        .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_iana_time_and_utc_as_iso_8601() {
        let out = run(json!({"timezone": "Europe/Istanbul"})).unwrap();
        let v: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(v["timezone"], "Europe/Istanbul");
        assert_eq!(v["utc_offset"], "+03:00");
        assert!(v["local"].as_str().unwrap().ends_with("+03:00"));
        assert!(v["utc"].as_str().unwrap().ends_with('Z'));
        assert!(matches!(
            v["weekday"].as_str().unwrap(),
            "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat" | "Sun"
        ));
    }

    #[test]
    fn refuses_unknown_timezone() {
        let e = run(json!({"timezone": "Mars/Olympus"})).unwrap_err();
        assert!(e.message.contains("unknown IANA timezone"));
    }
}
