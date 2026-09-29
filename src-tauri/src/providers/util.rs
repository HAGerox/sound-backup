use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn safe_stem(name: &str, fallback: &str) -> String {
    let mut output = String::new();
    let mut replaced = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_') {
            output.push(character);
            replaced = false;
        } else if !replaced {
            output.push('_');
            replaced = true;
        }
    }
    let output = output
        .trim_matches(|character: char| matches!(character, ' ' | '-' | '_'))
        .trim()
        .to_string();
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

pub fn dated_path(folder: &Path, name: &str, fallback: &str, extension: &str) -> PathBuf {
    dated_path_at(folder, name, fallback, extension, SystemTime::now())
}

fn dated_path_at(
    folder: &Path,
    name: &str,
    fallback: &str,
    extension: &str,
    when: SystemTime,
) -> PathBuf {
    folder.join(format!(
        "{}_{}.{}",
        utc_timestamp(when),
        safe_stem(name, fallback),
        extension.trim_start_matches('.')
    ))
}

pub fn utc_timestamp(when: SystemTime) -> String {
    let seconds = when
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}_{hour:02}-{minute:02}-{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn temporary_remote_path(label: &str, extension: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "/tmp/stage-backup-{}-{}-{}.{}",
        safe_stem(label, "backup").replace(' ', "-"),
        std::process::id(),
        nonce,
        extension.trim_start_matches('.')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn creates_safe_names_and_utc_dates() {
        assert_eq!(safe_stem("My: Show?.qlab5", "Project"), "My_ Show_qlab5");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(1_788_220_800)),
            "2026-09-01_00-00-00Z"
        );
        assert_eq!(
            dated_path_at(
                Path::new("/Backups/QLab"),
                "Big QLAB",
                "QLab Workspace",
                "zip",
                UNIX_EPOCH + Duration::from_secs(1_788_220_800),
            ),
            PathBuf::from("/Backups/QLab/2026-09-01_00-00-00Z_Big QLAB.zip")
        );
    }

    #[test]
    fn quotes_shell_strings() {
        assert_eq!(shell_quote("Operator's Mac"), "'Operator'\\''s Mac'");
    }
}
