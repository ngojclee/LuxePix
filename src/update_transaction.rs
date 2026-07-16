use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

const STAGING_PREFIX: &str = ".LuxePix-update-";
const RECOVERY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct PortableUpdateRequest {
    pub target_dir: PathBuf,
    pub archive_path: PathBuf,
    pub expected_sha256: String,
    pub expected_length: u64,
    pub wait_pid: u32,
    pub restart_executable: PathBuf,
    pub success_marker: PathBuf,
    pub handshake_token: String,
    pub startup_timeout: Duration,
}

#[derive(Debug)]
pub struct ReplacementTransaction {
    target_dir: PathBuf,
    staging_dir: PathBuf,
    backup_dir: PathBuf,
    recovery_path: PathBuf,
    installed_names: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct RecoveryState {
    schema_version: u32,
    target_dir: PathBuf,
    staging_dir: PathBuf,
    backup_dir: PathBuf,
    installed_names: Vec<PathBuf>,
}

pub fn run_portable_update(request: &PortableUpdateRequest) -> Result<(), String> {
    validate_handshake_token(&request.handshake_token)?;
    verify_file(
        &request.archive_path,
        request.expected_length,
        &request.expected_sha256,
    )?;
    wait_for_process_exit(request.wait_pid, Duration::from_secs(45))?;
    let transaction = replace_from_archive(
        &request.target_dir,
        &request.archive_path,
        &request.handshake_token,
    )?;

    let restart_result = restart_and_wait_for_handshake(request);
    match restart_result {
        Ok(()) => {
            transaction.commit()?;
            let _ = fs::remove_file(&request.archive_path);
            let _ = fs::remove_file(&request.success_marker);
            Ok(())
        }
        Err(error) => {
            transaction.rollback()?;
            let _ = Command::new(&request.restart_executable).spawn();
            Err(error)
        }
    }
}

pub fn verify_file(path: &Path, expected_length: u64, expected_sha256: &str) -> Result<(), String> {
    if expected_length == 0 {
        return Err("Expected update byte length cannot be zero.".into());
    }
    if !is_sha256(expected_sha256) {
        return Err("Expected update SHA-256 is invalid.".into());
    }
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() != expected_length {
        return Err(format!(
            "Update length mismatch: expected {expected_length}, got {}.",
            metadata.len()
        ));
    }
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err("Update SHA-256 verification failed.".into());
    }
    Ok(())
}

pub fn replace_from_archive(
    target_dir: &Path,
    archive_path: &Path,
    token: &str,
) -> Result<ReplacementTransaction, String> {
    validate_handshake_token(token)?;
    let target_dir = target_dir
        .canonicalize()
        .map_err(|error| format!("Could not resolve target directory: {error}"))?;
    if !target_dir.is_dir() {
        return Err("Portable update target is not a directory.".into());
    }
    let parent = target_dir
        .parent()
        .ok_or_else(|| "Portable update target cannot be a filesystem root.".to_string())?;
    let staging_dir = parent.join(format!("{STAGING_PREFIX}{token}-staging"));
    let backup_dir = parent.join(format!("{STAGING_PREFIX}{token}-backup"));
    let recovery_path = parent.join(format!("{STAGING_PREFIX}{token}-recovery.json"));
    validate_sibling_path(parent, &staging_dir)?;
    validate_sibling_path(parent, &backup_dir)?;
    validate_sibling_path(parent, &recovery_path)?;
    remove_transaction_path(&staging_dir, parent)?;
    remove_transaction_path(&backup_dir, parent)?;
    let _ = fs::remove_file(&recovery_path);
    fs::create_dir_all(&staging_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&backup_dir).map_err(|error| error.to_string())?;
    extract_archive_safely(archive_path, &staging_dir)?;
    if !staging_dir.join("LuxePix.exe").is_file() {
        remove_transaction_path(&staging_dir, parent)?;
        remove_transaction_path(&backup_dir, parent)?;
        return Err("Portable archive does not contain LuxePix.exe at its root.".into());
    }

    let mut transaction = ReplacementTransaction {
        target_dir,
        staging_dir,
        backup_dir,
        recovery_path,
        installed_names: Vec::new(),
    };
    if let Err(error) = transaction.apply() {
        let _ = transaction.rollback();
        return Err(error);
    }
    Ok(transaction)
}

impl ReplacementTransaction {
    fn apply(&mut self) -> Result<(), String> {
        let staged_entries: Vec<_> = fs::read_dir(&self.staging_dir)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        for entry in &staged_entries {
            let name = entry.file_name();
            if is_preserved_name(&name.to_string_lossy()) {
                continue;
            }
            let installed_path = self.target_dir.join(&name);
            if installed_path.exists() {
                fs::rename(&installed_path, self.backup_dir.join(&name)).map_err(|error| {
                    format!("Could not back up {}: {error}", installed_path.display())
                })?;
            }
        }

        for entry in staged_entries {
            let name = entry.file_name();
            if is_preserved_name(&name.to_string_lossy()) {
                continue;
            }
            fs::rename(entry.path(), self.target_dir.join(&name)).map_err(|error| {
                format!(
                    "Could not install staged item {}: {error}",
                    entry.path().display()
                )
            })?;
            self.installed_names.push(PathBuf::from(name));
        }
        write_recovery_state(
            &self.recovery_path,
            &RecoveryState {
                schema_version: RECOVERY_SCHEMA_VERSION,
                target_dir: self.target_dir.clone(),
                staging_dir: self.staging_dir.clone(),
                backup_dir: self.backup_dir.clone(),
                installed_names: self.installed_names.clone(),
            },
        )
    }

    pub fn commit(self) -> Result<(), String> {
        let parent = self
            .target_dir
            .parent()
            .ok_or_else(|| "Portable target has no parent directory.".to_string())?;
        remove_transaction_path(&self.backup_dir, parent)?;
        remove_transaction_path(&self.staging_dir, parent)?;
        let _ = fs::remove_file(self.recovery_path);
        Ok(())
    }

    pub fn rollback(self) -> Result<(), String> {
        let parent = self
            .target_dir
            .parent()
            .ok_or_else(|| "Portable target has no parent directory.".to_string())?;
        for relative in self.installed_names.iter().rev() {
            remove_installed_entry(&self.target_dir, relative)?;
        }
        if self.backup_dir.is_dir() {
            for entry in fs::read_dir(&self.backup_dir)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
            {
                fs::rename(entry.path(), self.target_dir.join(entry.file_name())).map_err(
                    |error| {
                        format!(
                            "Could not restore backup item {}: {error}",
                            entry.path().display()
                        )
                    },
                )?;
            }
        }
        remove_transaction_path(&self.backup_dir, parent)?;
        remove_transaction_path(&self.staging_dir, parent)?;
        let _ = fs::remove_file(self.recovery_path);
        Ok(())
    }
}

fn restart_and_wait_for_handshake(request: &PortableUpdateRequest) -> Result<(), String> {
    let _ = fs::remove_file(&request.success_marker);
    let mut child = Command::new(&request.restart_executable)
        .arg("--update-token")
        .arg(&request.handshake_token)
        .arg("--update-success")
        .arg(&request.success_marker)
        .spawn()
        .map_err(|error| format!("Could not restart LuxePix: {error}"))?;
    let deadline = Instant::now() + request.startup_timeout;
    while Instant::now() < deadline {
        if let Ok(value) = fs::read_to_string(&request.success_marker) {
            if value.trim() == request.handshake_token {
                return Ok(());
            }
        }
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("The updated LuxePix process exited before startup completed.".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    Err("The updated LuxePix process did not complete its startup handshake.".into())
}

fn extract_archive_safely(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("Unsafe path in update archive: {}", entry.name()))?
            .to_path_buf();
        if enclosed.as_os_str().is_empty()
            || enclosed.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!("Unsafe path in update archive: {}", entry.name()));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!("Symbolic links are not allowed: {}", entry.name()));
        }
        let output = destination.join(enclosed);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
        target.sync_all().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_recovery_state(path: &Path, state: &RecoveryState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    let temp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn validate_sibling_path(parent: &Path, path: &Path) -> Result<(), String> {
    if path.parent() != Some(parent)
        || !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX))
    {
        return Err(format!(
            "Unsafe update transaction path: {}",
            path.display()
        ));
    }
    Ok(())
}

fn remove_transaction_path(path: &Path, intended_parent: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.parent() != Some(intended_parent)
        || !path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(STAGING_PREFIX))
    {
        return Err(format!(
            "Refusing to remove unsafe path: {}",
            path.display()
        ));
    }
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    } else {
        fs::remove_file(path).map_err(|error| error.to_string())
    }
}

fn remove_installed_entry(target_dir: &Path, relative: &Path) -> Result<(), String> {
    if relative.components().count() != 1
        || !matches!(relative.components().next(), Some(Component::Normal(_)))
    {
        return Err(format!(
            "Refusing to remove unsafe installed path: {}",
            relative.display()
        ));
    }
    let path = target_dir.join(relative);
    if !path.exists() {
        return Ok(());
    }
    if path.parent() != Some(target_dir) {
        return Err(format!(
            "Refusing to remove path outside the portable target: {}",
            path.display()
        ));
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(&path).map_err(|error| error.to_string())
    } else {
        fs::remove_file(&path).map_err(|error| error.to_string())
    }
}

fn is_preserved_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("data") || name.eq_ignore_ascii_case(".portable")
}

fn validate_handshake_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > 96
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("Update handshake token is invalid.".into());
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(windows)]
fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    if pid == 0 {
        return Ok(());
    }
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) };
    let Ok(handle) = handle else {
        return Ok(());
    };
    let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
    let result = unsafe { WaitForSingleObject(handle, milliseconds) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else if result == WAIT_TIMEOUT {
        Err(format!(
            "LuxePix process {pid} did not exit before the update timeout."
        ))
    } else {
        Err(format!("Could not wait for LuxePix process {pid}."))
    }
}

#[cfg(not(windows))]
fn wait_for_process_exit(_pid: u32, _timeout: Duration) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{replace_from_archive, verify_file};
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "luxepix-update-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    fn create_archive(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).expect("create archive");
        let mut writer = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer
                .start_file(*name, options)
                .expect("start archive file");
            writer.write_all(bytes).expect("write archive file");
        }
        writer.finish().expect("finish archive");
    }

    #[test]
    fn portable_replacement_preserves_data_and_supports_rollback() {
        let root = test_root("rollback");
        let target = root.join("LuxePix");
        fs::create_dir_all(target.join("data")).expect("create target data");
        fs::write(target.join("LuxePix.exe"), b"old-app").expect("write old app");
        fs::write(target.join("LuxePixUpdater.exe"), b"old-updater").expect("write old updater");
        fs::write(target.join(".portable"), b"").expect("write marker");
        fs::write(target.join("data").join("config.json"), b"user-data").expect("write data");
        let archive = root.join("update.zip");
        create_archive(
            &archive,
            &[
                ("LuxePix.exe", b"new-app"),
                ("LuxePixUpdater.exe", b"new-updater"),
                (".portable", b"new-marker"),
                ("data/should-not-replace.txt", b"new-data"),
            ],
        );

        let transaction =
            replace_from_archive(&target, &archive, "rollback-test").expect("replace app");
        assert_eq!(
            fs::read(target.join("LuxePix.exe")).expect("read new app"),
            b"new-app"
        );
        assert_eq!(
            fs::read(target.join("data").join("config.json")).expect("read preserved data"),
            b"user-data"
        );
        assert!(!target.join("data").join("should-not-replace.txt").exists());

        transaction.rollback().expect("rollback app");
        assert_eq!(
            fs::read(target.join("LuxePix.exe")).expect("read restored app"),
            b"old-app"
        );
        assert_eq!(
            fs::read(target.join("data").join("config.json")).expect("read data after rollback"),
            b"user-data"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn archive_traversal_is_rejected_before_replacement() {
        let root = test_root("traversal");
        let target = root.join("LuxePix");
        fs::create_dir_all(&target).expect("create target");
        fs::write(target.join("LuxePix.exe"), b"old-app").expect("write old app");
        let archive = root.join("update.zip");
        create_archive(
            &archive,
            &[("LuxePix.exe", b"new-app"), ("../outside.txt", b"bad")],
        );

        assert!(replace_from_archive(&target, &archive, "traversal-test").is_err());
        assert!(!root.join("outside.txt").exists());
        assert_eq!(
            fs::read(target.join("LuxePix.exe")).expect("read untouched app"),
            b"old-app"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn file_verification_checks_length_and_sha256() {
        let root = test_root("verify");
        fs::create_dir_all(&root).expect("create fixture");
        let path = root.join("package.zip");
        fs::write(&path, b"verified bytes").expect("write package");
        let hash = format!("{:x}", Sha256::digest(b"verified bytes"));

        verify_file(&path, 14, &hash).expect("verify package");
        assert!(verify_file(&path, 13, &hash).is_err());
        assert!(verify_file(&path, 14, &"0".repeat(64)).is_err());
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
