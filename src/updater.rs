use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::{StorageMode, StoragePaths};
use crate::update_transaction;
use crate::version;

const RELEASE_MANIFEST_NAME: &str = "release-manifest.json";
const RELEASE_MANIFEST_SCHEMA: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);

pub const GITHUB_OWNER: &str = match option_env!("LUXEPIX_GITHUB_OWNER") {
    Some(value) => value,
    None => "ngojclee",
};
pub const GITHUB_REPOSITORY: &str = match option_env!("LUXEPIX_GITHUB_REPO") {
    Some(value) => value,
    None => "LuxePix",
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageMode {
    Portable,
    Installed,
}

impl PackageMode {
    pub const fn for_storage(mode: StorageMode) -> Self {
        match mode {
            StorageMode::Portable => Self::Portable,
            StorageMode::Installed => Self::Installed,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Portable => "Portable",
            Self::Installed => "Installed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub app_version: String,
    pub published_at: String,
    pub minimum_updater_version: String,
    pub entry_point: String,
    pub preserve: Vec<String>,
    #[serde(default)]
    pub model_pack_compatibility: Option<String>,
    pub release_notes_url: String,
    pub assets: Vec<ManifestAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestAsset {
    pub name: String,
    pub mode: PackageMode,
    pub architecture: Architecture,
    pub byte_length: u64,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedAsset {
    pub name: String,
    pub mode: PackageMode,
    pub byte_length: u64,
    pub sha256: String,
    pub download_url: String,
}

#[derive(Clone, Debug)]
pub struct UpdateOffer {
    pub version: Version,
    pub notes: String,
    pub published_at: String,
    pub release_url: String,
    pub asset: ResolvedAsset,
}

#[derive(Clone, Debug)]
pub struct VerifiedPackage {
    pub offer: UpdateOffer,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(UpdateOffer),
    Downloading { offer: UpdateOffer, downloaded: u64 },
    Ready(VerifiedPackage),
    Failed { message: String, manual: bool },
}

impl UpdateState {
    pub fn status_text(&self) -> String {
        match self {
            Self::Idle => "Not checked yet".into(),
            Self::Checking => "Checking GitHub Releases...".into(),
            Self::Current => format!("LuxePix {} is up to date", version::display()),
            Self::Available(offer) => format!("Version {} is available", offer.version),
            Self::Downloading { offer, downloaded } => format!(
                "Downloading {} of {}",
                format_bytes(*downloaded),
                format_bytes(offer.asset.byte_length)
            ),
            Self::Ready(package) => {
                format!("Version {} is verified and ready", package.offer.version)
            }
            Self::Failed { message, .. } => message.clone(),
        }
    }

    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Checking | Self::Downloading { .. })
    }
}

enum UpdateEvent {
    CheckFinished {
        generation: u64,
        manual: bool,
        result: Result<CheckResult, String>,
    },
    DownloadProgress {
        generation: u64,
        downloaded: u64,
    },
    DownloadFinished {
        generation: u64,
        offer: UpdateOffer,
        result: Result<PathBuf, String>,
    },
}

enum CheckResult {
    Current,
    Available(UpdateOffer),
}

pub struct UpdateManager {
    state: UpdateState,
    event_tx: mpsc::Sender<UpdateEvent>,
    event_rx: mpsc::Receiver<UpdateEvent>,
    cancellation: Option<Arc<AtomicBool>>,
    generation: u64,
}

impl Default for UpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel();
        Self {
            state: UpdateState::Idle,
            event_tx,
            event_rx,
            cancellation: None,
            generation: 0,
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn is_active(&self) -> bool {
        self.state.is_active()
    }

    pub fn active_count(&self) -> usize {
        usize::from(self.is_active())
    }

    pub fn start_check(&mut self, mode: StorageMode, manual: bool) -> Result<(), String> {
        if self.is_active() {
            return Err("An update task is already running.".into());
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        let generation = self.generation;
        self.state = UpdateState::Checking;
        self.cancellation = None;
        let tx = self.event_tx.clone();
        thread::Builder::new()
            .name("luxepix-update-check".into())
            .spawn(move || {
                let result = check_latest_release(PackageMode::for_storage(mode));
                let _ = tx.send(UpdateEvent::CheckFinished {
                    generation,
                    manual,
                    result,
                });
            })
            .map_err(|error| {
                self.state = UpdateState::Failed {
                    message: format!("Could not start update check: {error}"),
                    manual,
                };
                error.to_string()
            })?;
        Ok(())
    }

    pub fn start_download(&mut self, offer: UpdateOffer, updates_dir: &Path) -> Result<(), String> {
        if self.is_active() {
            return Err("An update task is already running.".into());
        }
        fs::create_dir_all(updates_dir).map_err(|error| error.to_string())?;
        self.generation = self.generation.wrapping_add(1).max(1);
        let generation = self.generation;
        let destination = updates_dir.join(&offer.asset.name);
        let cancellation = Arc::new(AtomicBool::new(false));
        self.cancellation = Some(Arc::clone(&cancellation));
        self.state = UpdateState::Downloading {
            offer: offer.clone(),
            downloaded: 0,
        };
        let tx = self.event_tx.clone();
        let thread_offer = offer.clone();
        thread::Builder::new()
            .name("luxepix-update-download".into())
            .spawn(move || {
                let result = download_and_verify(
                    &thread_offer.asset,
                    &destination,
                    &cancellation,
                    |downloaded| {
                        let _ = tx.send(UpdateEvent::DownloadProgress {
                            generation,
                            downloaded,
                        });
                    },
                );
                let _ = tx.send(UpdateEvent::DownloadFinished {
                    generation,
                    offer: thread_offer,
                    result,
                });
            })
            .map_err(|error| {
                self.state = UpdateState::Available(offer);
                self.cancellation = None;
                format!("Could not start update download: {error}")
            })?;
        Ok(())
    }

    pub fn cancel_download(&mut self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Relaxed);
        }
    }

    pub fn process_events(&mut self, limit: usize) -> bool {
        let mut changed = false;
        for _ in 0..limit {
            let event = match self.event_rx.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            changed = true;
            match event {
                UpdateEvent::CheckFinished {
                    generation,
                    manual,
                    result,
                } if generation == self.generation => {
                    self.cancellation = None;
                    self.state = match result {
                        Ok(CheckResult::Current) => UpdateState::Current,
                        Ok(CheckResult::Available(offer)) => UpdateState::Available(offer),
                        Err(message) => UpdateState::Failed { message, manual },
                    };
                }
                UpdateEvent::DownloadProgress {
                    generation,
                    downloaded,
                } if generation == self.generation => {
                    if let UpdateState::Downloading {
                        downloaded: current,
                        ..
                    } = &mut self.state
                    {
                        *current = downloaded;
                    }
                }
                UpdateEvent::DownloadFinished {
                    generation,
                    offer,
                    result,
                } if generation == self.generation => {
                    self.cancellation = None;
                    self.state = match result {
                        Ok(path) => UpdateState::Ready(VerifiedPackage { offer, path }),
                        Err(message) if message == "Update download cancelled." => {
                            UpdateState::Available(offer)
                        }
                        Err(message) => UpdateState::Failed {
                            message,
                            manual: true,
                        },
                    };
                }
                _ => {}
            }
        }
        changed
    }

    pub fn dismiss_offer(&mut self) {
        if matches!(
            self.state,
            UpdateState::Available(_) | UpdateState::Ready(_) | UpdateState::Failed { .. }
        ) {
            self.state = UpdateState::Idle;
        }
    }
}

#[derive(Clone, Debug)]
pub struct StartupHandshake {
    token: String,
    success_marker: PathBuf,
}

impl StartupHandshake {
    pub fn complete(self) -> Result<(), String> {
        validate_handshake_token(&self.token)?;
        let parent = self
            .success_marker
            .parent()
            .ok_or_else(|| "Update success marker has no parent directory.".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = self.success_marker.with_extension("tmp");
        fs::write(&temporary, self.token.as_bytes()).map_err(|error| error.to_string())?;
        fs::rename(&temporary, &self.success_marker).map_err(|error| error.to_string())
    }
}

pub fn startup_handshake_from_args() -> Result<Option<StartupHandshake>, String> {
    let mut token = None;
    let mut success_marker = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--update-token" {
            token = args.next().map(|value| value.to_string_lossy().to_string());
        } else if argument == "--update-success" {
            success_marker = args.next().map(PathBuf::from);
        }
    }
    match (token, success_marker) {
        (None, None) => Ok(None),
        (Some(token), Some(success_marker)) => {
            validate_handshake_token(&token)?;
            Ok(Some(StartupHandshake {
                token,
                success_marker,
            }))
        }
        _ => Err("Incomplete update startup handshake arguments.".into()),
    }
}

/// Inno Setup uninstall registry key (AppId + `_is1`) recorded by every
/// installed copy. Keep in sync with `AppId` in `installer/LuxePix.iss`.
pub const INNO_UNINSTALL_SUBKEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{FB281026-7847-4149-A551-8208EC55E64E}_is1";

pub fn launch_verified_package(
    package: &VerifiedPackage,
    storage: &StoragePaths,
) -> Result<(), String> {
    update_transaction::verify_file(
        &package.path,
        package.offer.asset.byte_length,
        &package.offer.asset.sha256,
    )?;
    match package.offer.asset.mode {
        PackageMode::Portable => launch_portable_update(package, storage),
        PackageMode::Installed => launch_installed_update(package, storage),
    }
}

pub fn repository_label() -> String {
    format!("{GITHUB_OWNER}/{GITHUB_REPOSITORY}")
}

fn launch_portable_update(package: &VerifiedPackage, storage: &StoragePaths) -> Result<(), String> {
    if storage.mode != StorageMode::Portable {
        return Err("A portable package cannot update an installed copy.".into());
    }
    let helper_source = storage.executable_dir.join("LuxePixUpdater.exe");
    if !helper_source.is_file() {
        return Err(format!(
            "Portable updater helper is missing: {}",
            helper_source.display()
        ));
    }
    let token = unique_token();
    let helper_dir = std::env::temp_dir().join(format!("LuxePixUpdater-{token}"));
    fs::create_dir_all(&helper_dir).map_err(|error| error.to_string())?;
    let helper_copy = helper_dir.join("LuxePixUpdater.exe");
    fs::copy(&helper_source, &helper_copy)
        .map_err(|error| format!("Could not prepare updater helper: {error}"))?;
    let success_marker = storage
        .updates_dir
        .join(format!("update-success-{token}.txt"));
    let restart_executable = storage.executable_dir.join("LuxePix.exe");

    Command::new(&helper_copy)
        .arg("--target-dir")
        .arg(&storage.executable_dir)
        .arg("--archive")
        .arg(&package.path)
        .arg("--sha256")
        .arg(&package.offer.asset.sha256)
        .arg("--length")
        .arg(package.offer.asset.byte_length.to_string())
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .arg("--restart-executable")
        .arg(restart_executable)
        .arg("--success-marker")
        .arg(success_marker)
        .arg("--token")
        .arg(token)
        .arg("--startup-timeout-ms")
        .arg("15000")
        .spawn()
        .map_err(|error| format!("Could not start portable updater: {error}"))?;
    Ok(())
}

fn launch_installed_update(
    package: &VerifiedPackage,
    storage: &StoragePaths,
) -> Result<(), String> {
    if package
        .path
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("exe"))
    {
        return Err("Installed update package must be an executable installer.".into());
    }
    let helper_source = storage.executable_dir.join("LuxePixUpdater.exe");
    if !helper_source.is_file() {
        // This copy was not deployed by LuxePix Setup (e.g. a dev build), so
        // hand the installer straight to the user with its full wizard.
        Command::new(&package.path)
            .arg("/LUXEPIXUPDATE=1")
            .spawn()
            .map_err(|error| format!("Could not start LuxePix Setup: {error}"))?;
        return Ok(());
    }
    let token = unique_token();
    let helper_dir = std::env::temp_dir().join(format!("LuxePixUpdater-{token}"));
    fs::create_dir_all(&helper_dir).map_err(|error| error.to_string())?;
    let helper_copy = helper_dir.join("LuxePixUpdater.exe");
    fs::copy(&helper_source, &helper_copy)
        .map_err(|error| format!("Could not prepare updater helper: {error}"))?;

    let uninstaller = storage.executable_dir.join("unins000.exe");
    let restart_executable = storage.executable_dir.join("LuxePix.exe");
    let mut command = Command::new(&helper_copy);
    command
        .arg("--installer")
        .arg(&package.path)
        .arg("--target-dir")
        .arg(&storage.executable_dir)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .arg("--restart-executable")
        .arg(restart_executable);
    if uninstaller.is_file() {
        command.arg("--uninstaller").arg(uninstaller);
    }
    if let Some(tasks) = inno_selected_tasks() {
        command.arg("--tasks").arg(tasks);
    }
    command
        .spawn()
        .map_err(|error| format!("Could not start the LuxePix update helper: {error}"))?;
    Ok(())
}

/// Best-effort read of the tasks selected during the current installation so
/// the reinstalled copy keeps the same shortcuts (e.g. the desktop icon).
#[cfg(windows)]
fn inno_selected_tasks() -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_SZ, REG_VALUE_TYPE,
    };

    let subkey: Vec<u16> = INNO_UNINSTALL_SUBKEY
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut hkey = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
    } != ERROR_SUCCESS
    {
        return None;
    }
    struct KeyGuard(HKEY);
    impl Drop for KeyGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
    let _guard = KeyGuard(hkey);

    let value_name: Vec<u16> = "Inno Setup: Selected Tasks"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut value_type = REG_VALUE_TYPE::default();
    let mut byte_len = 0u32;
    if unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut value_type),
            None,
            Some(&mut byte_len),
        )
    } != ERROR_SUCCESS
        || value_type != REG_SZ
        || byte_len == 0
        || byte_len > 4096
    {
        return None;
    }
    let mut buffer = vec![0u16; (byte_len as usize / 2) + 1];
    if unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut value_type),
            Some(buffer.as_mut_ptr() as *mut u8),
            Some(&mut byte_len),
        )
    } != ERROR_SUCCESS
    {
        return None;
    }
    let text = String::from_utf16_lossy(&buffer[..byte_len as usize / 2]);
    let tasks: Vec<&str> = text
        .trim_matches(char::from(0))
        .split(',')
        .map(str::trim)
        .filter(|task| {
            !task.is_empty()
                && task
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
        .collect();
    if tasks.is_empty() {
        None
    } else {
        Some(tasks.join(","))
    }
}

#[cfg(not(windows))]
fn inno_selected_tasks() -> Option<String> {
    None
}

fn check_latest_release(mode: PackageMode) -> Result<CheckResult, String> {
    if GITHUB_OWNER.trim().is_empty() || GITHUB_REPOSITORY.trim().is_empty() {
        return Err("The GitHub update repository is not configured.".into());
    }
    let release_url =
        format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPOSITORY}/releases/latest");
    let release: GitHubRelease = get_json(&release_url, MAX_MANIFEST_BYTES)?;
    if release.draft || release.prerelease {
        return Err("GitHub returned a draft or prerelease as the latest release.".into());
    }
    let release_version = version::parse_release_tag(&release.tag_name)
        .map_err(|error| format!("Invalid release tag {}: {error}", release.tag_name))?;
    let manifest_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == RELEASE_MANIFEST_NAME)
        .ok_or_else(|| "The release does not contain release-manifest.json.".to_string())?;
    if manifest_asset.size == 0 || manifest_asset.size > MAX_MANIFEST_BYTES {
        return Err("The release manifest has an invalid byte length.".into());
    }
    let manifest: ReleaseManifest =
        get_json(&manifest_asset.browser_download_url, MAX_MANIFEST_BYTES)?;
    let asset = validate_and_resolve_manifest(&manifest, &release, &release_version, mode)?;
    if release_version <= version::current() {
        return Ok(CheckResult::Current);
    }
    Ok(CheckResult::Available(UpdateOffer {
        version: release_version,
        notes: release.body.trim().to_string(),
        published_at: release
            .published_at
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| manifest.published_at.clone()),
        release_url: release.html_url,
        asset,
    }))
}

fn validate_and_resolve_manifest(
    manifest: &ReleaseManifest,
    release: &GitHubRelease,
    release_version: &Version,
    mode: PackageMode,
) -> Result<ResolvedAsset, String> {
    if manifest.schema_version != RELEASE_MANIFEST_SCHEMA {
        return Err(format!(
            "Unsupported release manifest schema {}.",
            manifest.schema_version
        ));
    }
    let manifest_version = Version::parse(manifest.app_version.trim())
        .map_err(|error| format!("Invalid manifest app version: {error}"))?;
    if &manifest_version != release_version {
        return Err("Release tag and manifest app version do not match.".into());
    }
    let minimum_updater = Version::parse(manifest.minimum_updater_version.trim())
        .map_err(|error| format!("Invalid minimum updater version: {error}"))?;
    if minimum_updater > version::current() {
        return Err(format!(
            "This release requires updater {}, but this copy is {}.",
            minimum_updater,
            version::current()
        ));
    }
    if manifest.entry_point != "LuxePix.exe" {
        return Err("The release manifest has an unexpected entry point.".into());
    }
    validate_preserve_entries(&manifest.preserve)?;
    if manifest.published_at.trim().is_empty() {
        return Err("The release manifest publication date is empty.".into());
    }
    if !manifest.release_notes_url.starts_with("https://") {
        return Err("The release manifest notes URL must use HTTPS.".into());
    }

    let candidates: Vec<&ManifestAsset> = manifest
        .assets
        .iter()
        .filter(|asset| asset.mode == mode && asset.architecture == Architecture::X86_64)
        .collect();
    if candidates.len() != 1 {
        return Err(format!(
            "Expected exactly one {} x64 asset in the release manifest.",
            mode.label()
        ));
    }
    let manifest_asset = candidates[0];
    validate_asset_name(&manifest_asset.name, mode)?;
    if manifest_asset.byte_length == 0 {
        return Err("The update package byte length cannot be zero.".into());
    }
    if !is_sha256(&manifest_asset.sha256) {
        return Err("The update package SHA-256 is invalid.".into());
    }
    let github_assets: Vec<&GitHubAsset> = release
        .assets
        .iter()
        .filter(|asset| asset.name == manifest_asset.name)
        .collect();
    if github_assets.len() != 1 {
        return Err("The manifest package is missing or duplicated in GitHub assets.".into());
    }
    let github_asset = github_assets[0];
    if github_asset.size != manifest_asset.byte_length {
        return Err("GitHub and manifest package byte lengths do not match.".into());
    }
    if !github_asset.browser_download_url.starts_with("https://") {
        return Err("The update package URL must use HTTPS.".into());
    }
    Ok(ResolvedAsset {
        name: manifest_asset.name.clone(),
        mode,
        byte_length: manifest_asset.byte_length,
        sha256: manifest_asset.sha256.to_ascii_lowercase(),
        download_url: github_asset.browser_download_url.clone(),
    })
}

fn validate_preserve_entries(entries: &[String]) -> Result<(), String> {
    if !entries
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case("data"))
        || !entries
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(".portable"))
    {
        return Err("Portable preservation rules must include data and .portable.".into());
    }
    for entry in entries {
        let path = Path::new(entry);
        if entry.trim().is_empty()
            || path.components().count() != 1
            || !matches!(path.components().next(), Some(Component::Normal(_)))
        {
            return Err(format!("Unsafe preservation entry: {entry}"));
        }
    }
    Ok(())
}

fn validate_asset_name(name: &str, mode: PackageMode) -> Result<(), String> {
    let path = Path::new(name);
    if name.trim().is_empty()
        || path.file_name() != Some(path.as_os_str())
        || name.contains(['/', '\\'])
    {
        return Err("The update package name is unsafe.".into());
    }
    let expected_suffix = match mode {
        PackageMode::Portable => "-windows-x64-portable.zip",
        PackageMode::Installed => "-windows-x64-setup.exe",
    };
    if !name.starts_with("LuxePix-v") || !name.ends_with(expected_suffix) {
        return Err(format!("Unexpected {} package name: {name}", mode.label()));
    }
    Ok(())
}

fn download_and_verify(
    asset: &ResolvedAsset,
    destination: &Path,
    cancellation: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, String> {
    let part_path = destination.with_extension(format!(
        "{}.part",
        destination
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ));
    let _ = fs::remove_file(&part_path);
    let response = request(&asset.download_url)?
        .call()
        .map_err(network_error)?;
    if let Some(length) = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
    {
        if length != asset.byte_length {
            return Err("Update server returned an unexpected byte length.".into());
        }
    }
    let mut reader = response.into_reader();
    let mut file = fs::File::create(&part_path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let result = (|| {
        loop {
            if cancellation.load(Ordering::Relaxed) {
                return Err("Update download cancelled.".to_string());
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("Update download failed: {error}"))?;
            if read == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(read as u64);
            if downloaded > asset.byte_length {
                return Err("Update download exceeded the published byte length.".into());
            }
            file.write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            hasher.update(&buffer[..read]);
            progress(downloaded);
        }
        file.sync_all().map_err(|error| error.to_string())?;
        if downloaded != asset.byte_length {
            return Err(format!(
                "Update length mismatch: expected {}, got {downloaded}.",
                asset.byte_length
            ));
        }
        let actual_hash = format!("{:x}", hasher.finalize());
        if !actual_hash.eq_ignore_ascii_case(&asset.sha256) {
            return Err("Update SHA-256 verification failed.".into());
        }
        Ok(())
    })();
    if let Err(error) = result {
        drop(file);
        let _ = fs::remove_file(&part_path);
        return Err(error);
    }
    drop(file);
    let _ = fs::remove_file(destination);
    fs::rename(&part_path, destination).map_err(|error| error.to_string())?;
    update_transaction::verify_file(destination, asset.byte_length, &asset.sha256)?;
    Ok(destination.to_path_buf())
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str, limit: u64) -> Result<T, String> {
    let response = request(url)?.call().map_err(network_error)?;
    let bytes = read_limited(response.into_reader(), limit)?;
    serde_json::from_slice(&bytes).map_err(|error| format!("Invalid release JSON: {error}"))
}

fn request(url: &str) -> Result<ureq::Request, String> {
    if !url.starts_with("https://") {
        return Err("Update requests require HTTPS.".into());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(NETWORK_TIMEOUT)
        .timeout_read(NETWORK_TIMEOUT)
        .timeout_write(NETWORK_TIMEOUT)
        .build();
    Ok(agent
        .get(url)
        .set("Accept", "application/vnd.github+json")
        .set(
            "User-Agent",
            &format!("LuxePix/{} (+https://github.com)", version::VERSION),
        )
        .set("X-GitHub-Api-Version", "2022-11-28"))
}

fn read_limited(mut reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Release metadata exceeds the allowed size.".into());
    }
    Ok(bytes)
}

fn network_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(403, _) => {
            "GitHub update check was rate-limited or access was denied.".into()
        }
        ureq::Error::Status(status, _) => {
            format!("GitHub update request failed with HTTP {status}.")
        }
        ureq::Error::Transport(error) => format!("Update service is unavailable: {error}"),
    }
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

fn unique_token() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{timestamp}", std::process::id())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: String,
    published_at: Option<String>,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        validate_and_resolve_manifest, Architecture, GitHubAsset, GitHubRelease, ManifestAsset,
        PackageMode, ReleaseManifest,
    };
    use semver::Version;

    fn release_and_manifest() -> (GitHubRelease, ReleaseManifest) {
        let portable_name = "LuxePix-v1.1.0-windows-x64-portable.zip";
        let setup_name = "LuxePix-v1.1.0-windows-x64-setup.exe";
        let release = GitHubRelease {
            tag_name: "v1.1.0".into(),
            draft: false,
            prerelease: false,
            body: "Release notes".into(),
            published_at: Some("2026-07-16T00:00:00Z".into()),
            html_url: "https://github.com/ngojclee/LuxePix/releases/tag/v1.1.0".into(),
            assets: vec![
                GitHubAsset {
                    name: portable_name.into(),
                    browser_download_url: "https://example.test/portable.zip".into(),
                    size: 100,
                },
                GitHubAsset {
                    name: setup_name.into(),
                    browser_download_url: "https://example.test/setup.exe".into(),
                    size: 200,
                },
            ],
        };
        let manifest = ReleaseManifest {
            schema_version: 1,
            app_version: "1.1.0".into(),
            published_at: "2026-07-16T00:00:00Z".into(),
            minimum_updater_version: "1.0.0".into(),
            entry_point: "LuxePix.exe".into(),
            preserve: vec!["data".into(), ".portable".into()],
            model_pack_compatibility: None,
            release_notes_url: release.html_url.clone(),
            assets: vec![
                ManifestAsset {
                    name: portable_name.into(),
                    mode: PackageMode::Portable,
                    architecture: Architecture::X86_64,
                    byte_length: 100,
                    sha256: "a".repeat(64),
                },
                ManifestAsset {
                    name: setup_name.into(),
                    mode: PackageMode::Installed,
                    architecture: Architecture::X86_64,
                    byte_length: 200,
                    sha256: "b".repeat(64),
                },
            ],
        };
        (release, manifest)
    }

    #[test]
    fn manifest_selects_exact_mode_and_architecture() {
        let (release, manifest) = release_and_manifest();
        let asset = validate_and_resolve_manifest(
            &manifest,
            &release,
            &Version::new(1, 1, 0),
            PackageMode::Portable,
        )
        .expect("resolve portable package");

        assert_eq!(asset.name, "LuxePix-v1.1.0-windows-x64-portable.zip");
        assert_eq!(asset.byte_length, 100);
    }

    #[test]
    fn manifest_rejects_release_version_mismatch() {
        let (release, manifest) = release_and_manifest();
        assert!(validate_and_resolve_manifest(
            &manifest,
            &release,
            &Version::new(1, 2, 0),
            PackageMode::Portable,
        )
        .is_err());
    }

    #[test]
    fn manifest_rejects_unsafe_preservation_rules() {
        let (release, mut manifest) = release_and_manifest();
        manifest.preserve.push("../outside".into());
        assert!(validate_and_resolve_manifest(
            &manifest,
            &release,
            &Version::new(1, 1, 0),
            PackageMode::Portable,
        )
        .is_err());
    }

    #[test]
    fn manifest_json_rejects_unknown_fields() {
        let json = serde_json::json!({
            "schema_version": 1,
            "app_version": "1.1.0",
            "published_at": "2026-07-16T00:00:00Z",
            "minimum_updater_version": "1.0.0",
            "entry_point": "LuxePix.exe",
            "preserve": ["data", ".portable"],
            "release_notes_url": "https://example.test/notes",
            "assets": [],
            "unexpected": true
        });
        assert!(serde_json::from_value::<ReleaseManifest>(json).is_err());
    }
}
