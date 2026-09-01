use std::{path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

pub const AVANTIS_USB_ROOT: &str = "AllenHeath-Avantis";
pub const AVANTIS_USB_SHOWS: &str = "Shows";

pub fn usb_show_directory(base: impl AsRef<Path>) -> PathBuf {
    base.as_ref().join(AVANTIS_USB_ROOT).join(AVANTIS_USB_SHOWS)
}

pub fn sanitise_archive_stem(name: &str) -> String {
    let without_extension = name
        .strip_suffix(".tar.gz")
        .or_else(|| name.strip_suffix(".tgz"))
        .or_else(|| name.strip_suffix(".gz"))
        .unwrap_or(name);

    let mut output = String::new();
    let mut last_was_replacement = false;
    for ch in without_extension.chars() {
        let accepted = ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_');
        if accepted {
            output.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            output.push('_');
            last_was_replacement = true;
        }
    }
    let output = output
        .trim_matches(|ch: char| matches!(ch, ' ' | '_' | '-'))
        .trim()
        .to_string();
    if output.is_empty() { "Show".into() } else { output }
}

pub fn dated_archive_name(source_name: &str, when: SystemTime) -> String {
    let stamp = utc_timestamp(when);
    format!("{}_{}.tar.gz", sanitise_archive_stem(source_name), stamp)
}

fn utc_timestamp(when: SystemTime) -> String {
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

// Gregorian conversion from days since Unix epoch. This keeps the protocol crate dependency-free.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn creates_avantis_usb_path() {
        assert_eq!(
            usb_show_directory("/tmp/backup"),
            PathBuf::from("/tmp/backup/AllenHeath-Avantis/Shows")
        );
    }

    #[test]
    fn filename_has_safe_symbols_and_date() {
        let name = dated_archive_name(
            "My: Show?.tar.gz",
            UNIX_EPOCH + Duration::from_secs(1_788_220_800),
        );
        assert_eq!(name, "My_ Show_2026-09-01_00-00-00Z.tar.gz");
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')));
    }
}
