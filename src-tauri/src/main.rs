mod providers;

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::Mutex};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct AvantisSettings {
    endpoint: String,
    selected_shows: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RemoteSettings {
    host: String,
    name: String,
    port: u16,
    username: String,
    fingerprint: String,
    local: bool,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            name: String::new(),
            port: 22,
            username: String::new(),
            fingerprint: String::new(),
            local: false,
        }
    }
}

impl From<&RemoteSettings> for providers::remote::Connection {
    fn from(settings: &RemoteSettings) -> Self {
        Self {
            host: settings.host.clone(),
            port: settings.port,
            username: settings.username.clone(),
            fingerprint: settings.fingerprint.clone(),
            local: settings.local,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct QLabInstanceSettings {
    host: String,
    address: String,
    hostname: String,
    name: String,
    osc_port: u16,
    local: bool,
}

impl Default for QLabInstanceSettings {
    fn default() -> Self {
        Self {
            host: String::new(),
            address: String::new(),
            hostname: String::new(),
            name: String::new(),
            osc_port: 53000,
            local: false,
        }
    }
}

impl From<&QLabInstanceSettings> for providers::qlab::Instance {
    fn from(settings: &QLabInstanceSettings) -> Self {
        Self {
            host: settings.host.clone(),
            address: settings.address.clone(),
            hostname: settings.hostname.clone(),
            name: settings.name.clone(),
            osc_port: settings.osc_port,
            local: settings.local,
            workspace_names: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct QLabWorkspaceSettings {
    id: String,
    name: String,
    port: u16,
    version: String,
}

impl From<&QLabWorkspaceSettings> for providers::qlab::Workspace {
    fn from(settings: &QLabWorkspaceSettings) -> Self {
        Self {
            id: settings.id.clone(),
            name: settings.name.clone(),
            port: settings.port,
            version: settings.version.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct QLabSettings {
    instance: QLabInstanceSettings,
    remote: RemoteSettings,
    selected_workspaces: Vec<QLabWorkspaceSettings>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct DeviceSettings {
    avantis: AvantisSettings,
    qlab: QLabSettings,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    backup_folder: String,
    devices: DeviceSettings,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct LegacySettings {
    console_address: String,
    show_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvantisBackupInput {
    console_address: String,
    show_names: Vec<String>,
    backup_folder: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteAccessInput {
    remote: RemoteSettings,
    password: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QLabWorkspacesInput {
    host: String,
    osc_port: u16,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QLabAuthoriseInput {
    instance: QLabInstanceSettings,
    workspace: QLabWorkspaceSettings,
    passcode: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QLabBackupInput {
    instance: QLabInstanceSettings,
    remote: RemoteSettings,
    workspaces: Vec<QLabWorkspaceSettings>,
    backup_folder: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupFileResponse {
    path: String,
    bytes: u64,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupResponse {
    files: Vec<BackupFileResponse>,
    total_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct AvantisDeviceResponse {
    endpoint: String,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
struct StoredShowResponse {
    name: String,
}

#[derive(Clone, Debug, Serialize)]
struct RemoteAccessResponse {
    fingerprint: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QLabInstanceResponse {
    host: String,
    address: String,
    hostname: String,
    name: String,
    osc_port: u16,
    local: bool,
    workspace_names: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct QLabWorkspaceResponse {
    id: String,
    name: String,
    port: u16,
    version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QLabAccessResponse {
    base_path: String,
}

#[derive(Default)]
struct AppState {
    settings_lock: Mutex<()>,
}

#[tauri::command]
fn load_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let path = settings_path(&app)?;
    match fs::read_to_string(path) {
        Ok(json) => decode_settings(&json),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Could not read settings: {error}")),
    }
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    let _guard = state
        .settings_lock
        .lock()
        .map_err(|_| "Could not lock the settings file.".to_string())?;
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the settings folder: {error}"))?;
    }
    let json = serde_json::to_vec_pretty(&settings)
        .map_err(|error| format!("Could not encode settings: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json).map_err(|error| format!("Could not save settings: {error}"))?;
    fs::rename(&temporary, &path).map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(())
}

#[tauri::command]
async fn choose_backup_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose backup folder")
        .blocking_pick_folder();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| "The selected folder is not a local filesystem path.".to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn open_remote_login_settings() -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.Sharing-Settings.extension")
        .status()
        .map_err(|error| format!("Could not open System Settings: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not open Sharing settings.".to_string())
    }
}

#[tauri::command]
async fn test_avantis(console_address: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || providers::avantis::test(&console_address))
        .await
        .map_err(|error| format!("Connection test stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn discover_avantis(
    known_address: Option<String>,
) -> Result<Vec<AvantisDeviceResponse>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::avantis::discover(known_address.as_deref())
    })
    .await
    .map_err(|error| format!("Avantis discovery stopped unexpectedly: {error}"))?
    .map(|devices| {
        devices
            .into_iter()
            .map(|device| AvantisDeviceResponse {
                endpoint: device.endpoint,
                name: "Avantis".to_string(),
            })
            .collect()
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn load_avantis_shows(console_address: String) -> Result<Vec<StoredShowResponse>, String> {
    tauri::async_runtime::spawn_blocking(move || providers::avantis::shows(&console_address))
        .await
        .map_err(|error| format!("Show discovery stopped unexpectedly: {error}"))?
        .map(|shows| {
            shows
                .into_iter()
                .map(|show| StoredShowResponse { name: show.name })
                .collect()
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn backup_avantis_shows(input: AvantisBackupInput) -> Result<BackupResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::avantis::backup(
            &input.console_address,
            input.show_names,
            PathBuf::from(&input.backup_folder),
        )
    })
    .await
    .map_err(|error| format!("Backup stopped unexpectedly: {error}"))?
    .map(|outcome| {
        let files: Vec<_> = outcome
            .files
            .into_iter()
            .map(|file| BackupFileResponse {
                path: file.path.to_string_lossy().into_owned(),
                bytes: file.bytes,
                name: file.show_name,
            })
            .collect();
        BackupResponse {
            total_bytes: files.iter().map(|file| file.bytes).sum(),
            files,
        }
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn test_remote_access(input: RemoteAccessInput) -> Result<RemoteAccessResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let connection = providers::remote::Connection::from(&input.remote);
        let connected = providers::remote::connect(&connection, input.password.as_deref())?;
        providers::remote::run(&connected.session, "/usr/bin/true")?;
        Ok(RemoteAccessResponse {
            fingerprint: connected.fingerprint,
        })
    })
    .await
    .map_err(|error| format!("Remote Login test stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn discover_qlab() -> Result<Vec<QLabInstanceResponse>, String> {
    let instances = tauri::async_runtime::spawn_blocking(providers::qlab::discover)
        .await
        .map_err(|error| format!("QLab discovery stopped unexpectedly: {error}"))??;
    Ok(instances
        .into_iter()
        .map(|instance| QLabInstanceResponse {
            host: instance.host,
            address: instance.address,
            hostname: instance.hostname,
            name: instance.name,
            osc_port: instance.osc_port,
            local: instance.local,
            workspace_names: instance.workspace_names,
        })
        .collect())
}

#[tauri::command]
async fn load_qlab_workspaces(
    input: QLabWorkspacesInput,
) -> Result<Vec<QLabWorkspaceResponse>, String> {
    let workspaces = tauri::async_runtime::spawn_blocking(move || {
        providers::qlab::list_workspaces(&input.host, input.osc_port)
    })
    .await
    .map_err(|error| format!("QLab workspace discovery stopped unexpectedly: {error}"))??;
    Ok(workspaces
        .into_iter()
        .map(|workspace| QLabWorkspaceResponse {
            id: workspace.id,
            name: workspace.name,
            port: workspace.port,
            version: workspace.version,
        })
        .collect())
}

#[tauri::command]
async fn authorise_qlab_workspace(input: QLabAuthoriseInput) -> Result<QLabAccessResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::qlab::authorise(
            &providers::qlab::Instance::from(&input.instance),
            &providers::qlab::Workspace::from(&input.workspace),
            input.passcode.as_deref(),
        )
        .map(|base_path| QLabAccessResponse { base_path })
    })
    .await
    .map_err(|error| format!("QLab access test stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn backup_qlab_workspaces(input: QLabBackupInput) -> Result<BackupResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let workspaces: Vec<_> = input
            .workspaces
            .iter()
            .map(providers::qlab::Workspace::from)
            .collect();
        providers::qlab::backup(
            &providers::qlab::Instance::from(&input.instance),
            &providers::remote::Connection::from(&input.remote),
            &workspaces,
            &PathBuf::from(input.backup_folder),
        )
    })
    .await
    .map_err(|error| format!("QLab backup stopped unexpectedly: {error}"))?
    .map(|outcomes| {
        let files: Vec<_> = outcomes
            .into_iter()
            .map(|outcome| BackupFileResponse {
                path: outcome.path.to_string_lossy().into_owned(),
                bytes: outcome.bytes,
                name: outcome.workspace_name,
            })
            .collect();
        BackupResponse {
            total_bytes: files.iter().map(|file| file.bytes).sum(),
            files,
        }
    })
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("settings.json"))
        .map_err(|error| format!("Could not locate the settings folder: {error}"))
}

fn decode_settings(json: &str) -> Result<Settings, String> {
    let mut settings: Settings =
        serde_json::from_str(json).map_err(|error| format!("Could not read settings: {error}"))?;
    let legacy: LegacySettings = serde_json::from_str(json).unwrap_or_default();
    if settings.devices.avantis.endpoint.is_empty() {
        settings.devices.avantis.endpoint = legacy.console_address;
    }
    if settings.devices.avantis.selected_shows.is_empty() && !legacy.show_name.is_empty() {
        settings
            .devices
            .avantis
            .selected_shows
            .push(legacy.show_name);
    }
    Ok(settings)
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            choose_backup_folder,
            open_remote_login_settings,
            test_avantis,
            discover_avantis,
            load_avantis_shows,
            backup_avantis_shows,
            test_remote_access,
            discover_qlab,
            load_qlab_workspaces,
            authorise_qlab_workspace,
            backup_qlab_workspaces
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Stage Backup");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_the_original_flat_settings_shape() {
        let settings = decode_settings(
            r#"{"consoleAddress":"192.168.1.50","showName":"Sunday","backupFolder":"/tmp"}"#,
        )
        .unwrap();
        assert_eq!(settings.backup_folder, "/tmp");
        assert_eq!(settings.devices.avantis.endpoint, "192.168.1.50");
        assert_eq!(
            settings.devices.avantis.selected_shows,
            vec!["Sunday".to_string()]
        );
        assert_eq!(settings.devices.qlab.instance.osc_port, 53000);
    }

    #[test]
    fn settings_never_contain_remote_passwords() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.to_ascii_lowercase().contains("password"));
        assert!(!json.to_ascii_lowercase().contains("passcode"));
    }
}
