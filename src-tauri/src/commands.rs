use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::connectors::{self, find_meta, validate_username};
use crate::scheduler::{self, AppState, TestResult};
use crate::secrets;
use crate::store::{Account, Settings};
use crate::streak::{in_lead_window, now_in, parse_clock, parse_tz, resolve};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAccount {
    pub id: Option<String>,
    pub connector_id: String,
    pub label: String,
    pub username: String,
    pub lead_hours: u32,
    pub timezone: String,
    pub enabled: bool,
    pub token: Option<String>,
    pub token_changed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAccount {
    pub account_id: Option<String>,
    pub connector_id: String,
    pub username: String,
    pub timezone: String,
    pub token: Option<String>,
    pub token_changed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    pub quiet_start: String,
    pub quiet_end: String,
    pub launch_at_login: bool,
}

#[tauri::command]
pub fn dashboard(app: AppHandle) -> Result<scheduler::Dashboard, String> {
    scheduler::dashboard(&app)
}

#[tauri::command]
pub fn catalog() -> Vec<connectors::ConnectorMeta> {
    connectors::catalog()
}

#[tauri::command]
pub fn active_reminder(app: AppHandle) -> Option<scheduler::Reminder> {
    scheduler::active_reminder(&app)
}

#[tauri::command]
pub async fn refresh_now(app: AppHandle) -> Result<scheduler::Dashboard, String> {
    scheduler::run_checks(&app).await;
    scheduler::dashboard(&app)
}

#[tauri::command]
pub async fn test_connector(app: AppHandle, input: TestAccount) -> Result<TestResult, String> {
    let username = validate_username(&input.username)?;
    find_meta(&input.connector_id).ok_or("That connector isn't available.")?;
    let tz = parse_tz(&input.timezone)?;
    let stored = if input.token_changed {
        None
    } else if let Some(id) = input.account_id.as_deref() {
        secrets::get_token(id)?
    } else {
        None
    };
    let token = if input.token_changed {
        input.token.as_deref().map(str::trim).filter(|token| !token.is_empty())
    } else {
        stored.as_deref()
    };
    let http = app.state::<AppState>().http.clone();
    match connectors::check(&http, &input.connector_id, &username, token, tz).await {
        Ok(activity) => {
            let now = now_in(tz);
            let resolution = resolve(
                &activity.days,
                activity.reported_streak,
                activity.last_active,
                now.date_naive(),
                in_lead_window(now, 3),
            );
            Ok(TestResult {
                ok: true,
                summary: scheduler::summary_for(&resolution),
            })
        }
        Err(err) => Ok(TestResult {
            ok: false,
            summary: err,
        }),
    }
}

#[tauri::command]
pub async fn save_account(app: AppHandle, input: SaveAccount) -> Result<scheduler::AccountView, String> {
    let meta = find_meta(&input.connector_id).ok_or("That connector isn't available.")?;
    let username = validate_username(&input.username)?;
    if !(1..=24).contains(&input.lead_hours) {
        return Err("Remind between 1 and 24 hours before midnight.".into());
    }
    if !input.timezone.trim().is_empty() {
        parse_tz(&input.timezone)?;
    }
    let id = input
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let label = {
        let trimmed = input.label.trim();
        if trimmed.is_empty() {
            format!("{} · {username}", meta.name)
        } else {
            trimmed.chars().take(60).collect()
        }
    };
    let account = Account {
        id: id.clone(),
        connector_id: input.connector_id,
        label,
        username,
        lead_hours: input.lead_hours,
        enabled: input.enabled,
        timezone: input.timezone.trim().to_string(),
    };
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        if input.id.as_ref().is_some_and(|existing| !existing.is_empty())
            && db.get_account(&id)?.is_none()
        {
            return Err("That streak is no longer saved.".into());
        }
        db.upsert_account(&account)?;
    }
    if input.token_changed {
        match input.token.as_deref().map(str::trim).filter(|token| !token.is_empty()) {
            Some(token) => secrets::set_token(&id, token)?,
            None => secrets::delete_token(&id)?,
        }
    }
    scheduler::check_enabled(&app, Some(&id)).await?;
    scheduler::reevaluate(&app);
    let _ = app.emit("streaks-updated", ());
    scheduler::dashboard(&app)?
        .accounts
        .into_iter()
        .find(|view| view.id == id)
        .ok_or_else(|| "Saved the streak, but it didn't show up.".to_string())
}

#[tauri::command]
pub fn delete_account(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        db.delete_account(&id)?;
    }
    secrets::delete_token(&id)?;
    scheduler::reevaluate(&app);
    let _ = app.emit("streaks-updated", ());
    Ok(())
}

#[tauri::command]
pub fn set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        db.set_enabled(&id, enabled)?;
    }
    scheduler::reevaluate(&app);
    let _ = app.emit("streaks-updated", ());
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().expect("db");
    db.settings()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, input: SettingsInput) -> Result<Settings, String> {
    let start = input.quiet_start.trim();
    let end = input.quiet_end.trim();
    let (quiet_start, quiet_end) = match (start.is_empty(), end.is_empty()) {
        (true, true) => (None, None),
        (false, false) => {
            let start_clock = parse_clock(start)?;
            let end_clock = parse_clock(end)?;
            if start_clock == end_clock {
                return Err("Quiet hours need a start and an end that differ.".into());
            }
            (Some(start.to_string()), Some(end.to_string()))
        }
        _ => return Err("Set both quiet hours, or leave both blank.".into()),
    };
    let settings = Settings {
        quiet_start,
        quiet_end,
        launch_at_login: input.launch_at_login,
    };
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        db.save_settings(&settings)?;
    }
    let auto = app.autolaunch();
    let result = if input.launch_at_login {
        auto.enable()
    } else {
        auto.disable()
    };
    if let Err(err) = result {
        eprintln!("launch at login: {err}");
    }
    scheduler::reevaluate(&app);
    let _ = app.emit("streaks-updated", ());
    Ok(settings)
}

#[tauri::command]
pub fn snooze(app: AppHandle, account_id: String, minutes: i64) -> Result<(), String> {
    if !(1..=180).contains(&minutes) {
        return Err("Snooze for a few minutes, up to three hours.".into());
    }
    scheduler::snooze(&app, &account_id, minutes)?;
    let _ = app.emit("streaks-updated", ());
    Ok(())
}

#[tauri::command]
pub fn dismiss_reminder(app: AppHandle, account_id: String) -> Result<(), String> {
    scheduler::dismiss(&app, &account_id)?;
    let _ = app.emit("streaks-updated", ());
    Ok(())
}

#[tauri::command]
pub fn open_service(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("That link isn't one this app can open.".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| format!("Couldn't open the link: {err}"))
}

#[tauri::command]
pub fn show_settings(app: AppHandle) {
    if let Some(window) = app.get_webview_window("popover") {
        let _ = window.hide();
    }
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.center();
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

pub fn apply_launch_login(app: &AppHandle, enabled: bool) {
    let auto = app.autolaunch();
    let result = if enabled { auto.enable() } else { auto.disable() };
    if let Err(err) = result {
        eprintln!("launch at login: {err}");
    }
}
