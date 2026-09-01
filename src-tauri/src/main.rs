mod providers;

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct AvantisSettings {
    endpoint: String,
    selected_shows: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct DeviceSettings {
    avantis: AvantisSettings,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    backup_folder: String,
    #[serde(default)]
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
struct BackupInput {
    console_address: String,
    show_names: Vec<String>,
    backup_folder: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupFileResponse {
    path: String,
    bytes: u64,
    show_name: String,
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
async fn backup_avantis_shows(input: BackupInput) -> Result<BackupResponse, String> {
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
        let total_bytes = outcome.files.iter().map(|file| file.bytes).sum();
        BackupResponse {
            files: outcome
                .files
                .into_iter()
                .map(|file| BackupFileResponse {
                    path: file.path.to_string_lossy().into_owned(),
                    bytes: file.bytes,
                    show_name: file.show_name,
                })
                .collect(),
            total_bytes,
        }
    })
    .map_err(|error| error.to_string())
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
            test_avantis,
            discover_avantis,
            load_avantis_shows,
            backup_avantis_shows
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
    }
}
