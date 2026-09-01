mod providers;

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    console_address: String,
    show_name: String,
    backup_folder: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupInput {
    console_address: String,
    show_name: String,
    backup_folder: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupResponse {
    path: String,
    bytes: u64,
    show_name: String,
}

#[derive(Default)]
struct AppState {
    settings_lock: Mutex<()>,
}

#[tauri::command]
fn load_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let path = settings_path(&app)?;
    match fs::read_to_string(path) {
        Ok(json) => {
            serde_json::from_str(&json).map_err(|error| format!("Could not read settings: {error}"))
        }
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
async fn backup_avantis_show(input: BackupInput) -> Result<BackupResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        providers::avantis::backup(
            &input.console_address,
            &input.show_name,
            PathBuf::from(&input.backup_folder),
        )
    })
    .await
    .map_err(|error| format!("Backup stopped unexpectedly: {error}"))?
    .map(|outcome| BackupResponse {
        path: outcome.path.to_string_lossy().into_owned(),
        bytes: outcome.bytes,
        show_name: outcome.show_name,
    })
    .map_err(|error| error.to_string())
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join("settings.json"))
        .map_err(|error| format!("Could not locate the settings folder: {error}"))
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
            backup_avantis_show
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Stage Backup");
}
