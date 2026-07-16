#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

#[allow(dead_code)]
#[path = "../update_transaction.rs"]
mod update_transaction;

use update_transaction::PortableUpdateRequest;

fn main() {
    if let Err(error) = run() {
        let _ = rfd::MessageDialog::new()
            .set_title("LuxePix Update Failed")
            .set_description(&error)
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let arguments = parse_arguments(std::env::args_os().skip(1))?;
    let request = PortableUpdateRequest {
        target_dir: required_path(&arguments, "--target-dir")?,
        archive_path: required_path(&arguments, "--archive")?,
        expected_sha256: required_string(&arguments, "--sha256")?,
        expected_length: required_number(&arguments, "--length")?,
        wait_pid: required_number(&arguments, "--wait-pid")?,
        restart_executable: required_path(&arguments, "--restart-executable")?,
        success_marker: required_path(&arguments, "--success-marker")?,
        handshake_token: required_string(&arguments, "--token")?,
        startup_timeout: Duration::from_millis(required_number(
            &arguments,
            "--startup-timeout-ms",
        )?),
    };
    update_transaction::run_portable_update(&request)
}

fn parse_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<HashMap<String, OsString>, String> {
    let values: Vec<OsString> = arguments.collect();
    if !values.len().is_multiple_of(2) {
        return Err("Updater arguments must be supplied as name/value pairs.".into());
    }
    let mut parsed = HashMap::new();
    for pair in values.chunks_exact(2) {
        let name = pair[0].to_string_lossy().to_string();
        if !name.starts_with("--") || parsed.insert(name.clone(), pair[1].clone()).is_some() {
            return Err(format!("Invalid or duplicated updater argument: {name}"));
        }
    }
    Ok(parsed)
}

fn required_path(arguments: &HashMap<String, OsString>, name: &str) -> Result<PathBuf, String> {
    arguments
        .get(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing updater argument {name}."))
}

fn required_string(arguments: &HashMap<String, OsString>, name: &str) -> Result<String, String> {
    arguments
        .get(name)
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Missing updater argument {name}."))
}

fn required_number<T>(arguments: &HashMap<String, OsString>, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    required_string(arguments, name)?
        .parse()
        .map_err(|_| format!("Updater argument {name} is not a valid number."))
}

#[cfg(test)]
mod tests {
    use super::parse_arguments;
    use std::ffi::OsString;

    #[test]
    fn updater_arguments_reject_duplicates() {
        let values = vec![
            OsString::from("--target-dir"),
            OsString::from("one"),
            OsString::from("--target-dir"),
            OsString::from("two"),
        ];
        assert!(parse_arguments(values.into_iter()).is_err());
    }
}
