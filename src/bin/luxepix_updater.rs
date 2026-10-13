#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
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
    if arguments.contains_key("--installer") {
        return run_installed_update(&arguments);
    }
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

/// Installed-mode handoff: wait for LuxePix to exit, silently remove the old
/// Inno Setup installation (settings in AppData are kept because the
/// uninstaller runs with /SUPPRESSMSGBOXES), then run the downloaded setup
/// executable with its full wizard so the new version goes back into the same
/// folder with the same tasks.
fn run_installed_update(arguments: &HashMap<String, OsString>) -> Result<(), String> {
    let installer = required_path(arguments, "--installer")?;
    let target_dir = required_path(arguments, "--target-dir")?;
    let wait_pid: u32 = required_number(arguments, "--wait-pid")?;
    let uninstaller = arguments
        .get("--uninstaller")
        .map(PathBuf::from)
        .unwrap_or_else(|| target_dir.join("unins000.exe"));
    let tasks = optional_string(arguments, "--tasks").unwrap_or_default();
    let restart_executable = arguments
        .get("--restart-executable")
        .map(PathBuf::from)
        .unwrap_or_else(|| target_dir.join("LuxePix.exe"));

    if installer
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("exe"))
    {
        return Err("The downloaded update package is not an installer.".into());
    }
    if !installer.is_file() {
        return Err(format!(
            "The downloaded installer is missing: {}",
            installer.display()
        ));
    }

    update_transaction::wait_for_process_exit_pub(wait_pid, Duration::from_secs(45))?;

    let mut uninstalled = false;
    if uninstaller.is_file() {
        let status = Command::new(&uninstaller)
            .arg("/VERYSILENT")
            .arg("/SUPPRESSMSGBOXES")
            .arg("/NORESTART")
            .status()
            .map_err(|error| {
                format!(
                    "Could not start the LuxePix uninstaller {}: {error}",
                    uninstaller.display()
                )
            })?;
        if !status.success() {
            relaunch_existing(&restart_executable);
            return Err(
                "The uninstaller did not finish; the installed copy was left unchanged.".into(),
            );
        }
        uninstalled = true;
    }

    let mut setup = Command::new(&installer);
    setup
        .arg(format!("/DIR={}", target_dir.to_string_lossy()))
        .arg("/LUXEPIXUPDATE=1");
    if !tasks.is_empty() {
        setup.arg(format!("/TASKS={tasks}"));
    }
    let status = setup.status().map_err(|error| {
        format!(
            "Could not start LuxePix Setup {}: {error}",
            installer.display()
        )
    })?;

    if status.success() && restart_executable.is_file() {
        let _ = std::fs::remove_file(&installer);
        return Ok(());
    }

    if !uninstalled && restart_executable.is_file() {
        relaunch_existing(&restart_executable);
        return Err(
            "Setup was cancelled; the installed copy was left unchanged and has been restarted."
                .into(),
        );
    }
    Err(format!(
        "Setup did not finish installing LuxePix. The downloaded installer remains at {} \
         if you want to run it manually. Your settings, cache, and product index in AppData \
         were preserved.",
        installer.display()
    ))
}

fn relaunch_existing(executable: &std::path::Path) {
    if executable.is_file() {
        let _ = Command::new(executable).spawn();
    }
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

fn optional_string(arguments: &HashMap<String, OsString>, name: &str) -> Option<String> {
    arguments
        .get(name)
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
}

fn required_string(arguments: &HashMap<String, OsString>, name: &str) -> Result<String, String> {
    optional_string(arguments, name).ok_or_else(|| format!("Missing updater argument {name}."))
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
