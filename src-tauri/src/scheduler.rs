use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use tauri::{image::Image, tray::TrayIcon, AppHandle, Emitter, Manager};

use crate::connectors::{self, find_meta, service_url};
use crate::secrets;
use crate::store::{Account, Snapshot, Store};
use crate::streak::{
    format_time_left, fresh_break, in_lead_window, mark_shown, minutes_until_midnight, notice_for,
    now_in, parse_tz, quiet_now, resolve, system_tz, tray_rank, NoticeGate, NoticeKind, Resolution,
    Status,
};

pub struct AppState {
    pub db: std::sync::Mutex<Store>,
    pub http: reqwest::Client,
    pub tray: std::sync::Mutex<Option<TrayIcon>>,
    pub active: std::sync::Mutex<Option<Reminder>>,
    pub last_check: std::sync::Mutex<Option<Instant>>,
    pub checking: std::sync::Mutex<bool>,
    pub popover_grace: std::sync::Mutex<Instant>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub connector_id: String,
    pub connector_name: String,
    pub label: String,
    pub username: String,
    pub lead_hours: u32,
    pub enabled: bool,
    pub timezone: String,
    pub has_token: bool,
    pub streak: u32,
    pub done_today: bool,
    pub status: String,
    pub detail: String,
    pub error: Option<String>,
    pub checked_at: Option<String>,
    pub service_url: String,
    pub time_left: Option<String>,
    pub fresh_break: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub accounts: Vec<AccountView>,
    pub next_check_at: Option<String>,
    pub checking: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub account_id: String,
    pub connector_name: String,
    pub label: String,
    pub kind: String,
    pub message: String,
    pub service_url: String,
    pub streak: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub summary: String,
}

struct Prepared {
    account: Account,
    view: AccountView,
    resolution: Option<Resolution>,
    today: NaiveDate,
    gate: NoticeGate,
    quiet: bool,
    checked_today: bool,
}

const CHECK_EVERY: Duration = Duration::from_secs(45 * 60);
const TICK: Duration = Duration::from_secs(60);

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        run_checks(&app).await;
        loop {
            let started = Instant::now();
            tokio::time::sleep(TICK).await;
            let woke = started.elapsed() > Duration::from_secs(90);
            let due = {
                let state = app.state::<AppState>();
                let last_check = state.last_check.lock().expect("last check");
                last_check
                    .map(|at| at.elapsed() >= CHECK_EVERY)
                    .unwrap_or(true)
            };
            if woke || due || day_is_stale(&app) {
                run_checks(&app).await;
            } else {
                reevaluate(&app);
                let _ = app.emit("streaks-updated", ());
            }
        }
    });
}

pub async fn run_checks(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut checking = state.checking.lock().expect("checking");
        if *checking {
            return;
        }
        *checking = true;
    }
    let _ = app.emit("streaks-updated", ());
    if let Err(err) = check_enabled(app, None).await {
        eprintln!("streak check failed: {err}");
    }
    {
        let state = app.state::<AppState>();
        *state.checking.lock().expect("checking") = false;
        *state.last_check.lock().expect("last check") = Some(Instant::now());
    }
    reevaluate(app);
    let _ = app.emit("streaks-updated", ());
}

pub async fn check_enabled(app: &AppHandle, only: Option<&str>) -> Result<(), String> {
    let jobs = {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        let mut jobs = Vec::new();
        for account in db.list_accounts()? {
            if !account.enabled {
                continue;
            }
            if only.is_some_and(|id| id != account.id) {
                continue;
            }
            match secrets::get_token(&account.id) {
                Ok(token) => jobs.push((account, Ok(token))),
                Err(err) => jobs.push((account, Err(err))),
            }
        }
        jobs
    };
    let http = app.state::<AppState>().http.clone();
    let checks = jobs.into_iter().map(|(account, token)| {
        let http = http.clone();
        async move {
            let previous_id = account.id.clone();
            let outcome = match token {
                Ok(token) => check_account(&http, &account, token.as_deref()).await,
                Err(err) => Err(err),
            };
            (previous_id, account, outcome)
        }
    });
    let results = futures::future::join_all(checks).await;
    let state = app.state::<AppState>();
    let db = state.db.lock().expect("db");
    for (id, account, outcome) in results {
        let snapshot = match outcome {
            Ok((resolution, url)) => success_snapshot(&resolution, url),
            Err(err) => {
                eprintln!("check {} ({}) failed: {err}", account.label, account.connector_id);
                let previous = db.snapshot(&id)?;
                error_snapshot(previous, err, service_url(&account.connector_id, &account.username))
            }
        };
        db.save_snapshot(&id, &snapshot)?;
    }
    Ok(())
}

async fn check_account(
    client: &reqwest::Client,
    account: &Account,
    token: Option<&str>,
) -> Result<(Resolution, String), String> {
    let tz = account_tz(&account.timezone)?;
    let now = now_in(tz);
    let activity = connectors::check(
        client,
        &account.connector_id,
        &account.username,
        token,
        tz,
    )
    .await?;
    let resolution = resolve(
        &activity.days,
        activity.reported_streak,
        activity.last_active,
        now.date_naive(),
        in_lead_window(now, account.lead_hours),
    );
    Ok((resolution, activity.service_url))
}

pub fn reevaluate(app: &AppHandle) {
    let prepared = match prepare(app) {
        Ok(prepared) => prepared,
        Err(err) => {
            eprintln!("couldn't read streaks: {err}");
            return;
        }
    };
    paint_tray(app, &prepared);
    let state = app.state::<AppState>();
    let active = state.active.lock().expect("active").clone();
    if let Some(current) = active {
        if keep_current(&prepared, &current) {
            return;
        }
        drop(state);
        clear_reminder(app);
    }
    let Some(next) = pick_notice(&prepared) else {
        return;
    };
    if let Err(err) = persist_notice(app, &next) {
        eprintln!("couldn't remember a reminder: {err}");
        return;
    }
    present_reminder(app, next.reminder);
}

pub fn dashboard(app: &AppHandle) -> Result<Dashboard, String> {
    let prepared = prepare(app)?;
    let state = app.state::<AppState>();
    let checking = *state.checking.lock().expect("checking");
    let next_check_at = state.last_check.lock().expect("last check").map(|at| {
        let remaining = CHECK_EVERY.saturating_sub(at.elapsed());
        (Utc::now() + chrono::Duration::from_std(remaining).unwrap_or_default()).to_rfc3339()
    });
    Ok(Dashboard {
        accounts: prepared.into_iter().map(|item| item.view).collect(),
        next_check_at,
        checking,
    })
}

pub fn active_reminder(app: &AppHandle) -> Option<Reminder> {
    app.state::<AppState>()
        .active
        .lock()
        .expect("active")
        .clone()
}

pub fn snooze(app: &AppHandle, account_id: &str, minutes: i64) -> Result<(), String> {
    let until = Utc::now() + chrono::Duration::minutes(minutes);
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        let mut gate = db.gate(account_id)?;
        gate.snooze_until = Some(until);
        db.save_gate(account_id, &gate)?;
    }
    clear_reminder(app);
    reevaluate(app);
    Ok(())
}

pub fn dismiss(app: &AppHandle, account_id: &str) -> Result<(), String> {
    let prepared = prepare(app)?;
    let item = prepared
        .iter()
        .find(|item| item.account.id == account_id)
        .ok_or("That streak is no longer saved.")?;
    {
        let state = app.state::<AppState>();
        let db = state.db.lock().expect("db");
        let mut gate = db.gate(account_id)?;
        gate.risk_dismissed_on = Some(item.today);
        if item.view.status == "ended" {
            gate.ended_notified_for = item
                .resolution
                .as_ref()
                .and_then(|resolution| resolution.last_active);
        }
        db.save_gate(account_id, &gate)?;
    }
    clear_reminder(app);
    reevaluate(app);
    Ok(())
}

pub fn summary_for(resolution: &Resolution) -> String {
    if resolution.done_today {
        format!("{}-day streak · done today", resolution.streak)
    } else if resolution.streak > 0 {
        format!("{}-day streak · not done today", resolution.streak)
    } else {
        resolution.detail.clone()
    }
}

fn prepare(app: &AppHandle) -> Result<Vec<Prepared>, String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().expect("db");
    let settings = db.settings()?;
    let (quiet_start, quiet_end) = settings.quiet_bounds()?;
    let mut prepared = Vec::new();
    for account in db.list_accounts()? {
        let tz = account_tz(&account.timezone).unwrap_or_else(|_| system_tz());
        let now = now_in(tz);
        let today = now.date_naive();
        let snapshot = db.snapshot(&account.id)?;
        let has_token = secrets::get_token(&account.id).ok().flatten().is_some();
        let quiet = quiet_now(now, quiet_start, quiet_end);
        let (view, resolution, checked_today) =
            describe(&account, snapshot.as_ref(), tz, now, has_token);
        prepared.push(Prepared {
            gate: db.gate(&account.id)?,
            account,
            view,
            resolution,
            today,
            quiet,
            checked_today,
        });
    }
    Ok(prepared)
}

fn describe(
    account: &Account,
    snapshot: Option<&Snapshot>,
    tz: Tz,
    now: DateTime<Tz>,
    has_token: bool,
) -> (AccountView, Option<Resolution>, bool) {
    let connector_name = find_meta(&account.connector_id)
        .map(|meta| meta.name)
        .unwrap_or_else(|| "Streak".to_string());
    let base = AccountView {
        id: account.id.clone(),
        connector_id: account.connector_id.clone(),
        connector_name,
        label: account.label.clone(),
        username: account.username.clone(),
        lead_hours: account.lead_hours,
        enabled: account.enabled,
        timezone: account.timezone.clone(),
        has_token,
        streak: 0,
        done_today: false,
        status: "unchecked".to_string(),
        detail: "Not checked yet".to_string(),
        error: None,
        checked_at: None,
        service_url: service_url(&account.connector_id, &account.username),
        time_left: None,
        fresh_break: false,
    };
    let Some(snapshot) = snapshot else {
        return (base, None, false);
    };
    let mut view = base;
    view.streak = snapshot.streak;
    view.done_today = snapshot.done_today;
    view.checked_at = snapshot.checked_at.clone();
    view.service_url = snapshot
        .service_url
        .clone()
        .filter(|url| url.starts_with("https://"))
        .unwrap_or(view.service_url);
    let checked_today = snapshot.checked_at.as_deref().is_some_and(|text| {
        DateTime::parse_from_rfc3339(text)
            .ok()
            .is_some_and(|dt| dt.with_timezone(&tz).date_naive() == now.date_naive())
    });
    if snapshot.status == "error" {
        view.status = "error".to_string();
        view.error = snapshot.error.clone();
        view.detail = snapshot
            .error
            .clone()
            .unwrap_or_else(|| "Couldn't check".to_string());
        return (view, None, checked_today);
    }
    let last_active = snapshot.last_active.as_deref().and_then(|text| {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
    });
    let resolution = resolve(
        &[],
        Some(snapshot.streak),
        last_active,
        now.date_naive(),
        in_lead_window(now, account.lead_hours),
    );
    view.status = resolution.status.as_str().to_string();
    view.detail = resolution.detail.clone();
    view.streak = resolution.streak;
    view.done_today = resolution.done_today;
    view.fresh_break = resolution.status == Status::Ended && fresh_break(last_active, now.date_naive());
    if matches!(resolution.status, Status::Waiting | Status::AtRisk) {
        view.time_left = Some(format_time_left(minutes_until_midnight(now)));
    }
    (view, Some(resolution), checked_today)
}

fn keep_current(prepared: &[Prepared], current: &Reminder) -> bool {
    let Some(item) = prepared.iter().find(|item| item.account.id == current.account_id) else {
        return false;
    };
    if !item.account.enabled {
        return false;
    }
    let Some(resolution) = &item.resolution else {
        return false;
    };
    match current.kind.as_str() {
        "at_risk" => {
            resolution.status == Status::AtRisk
                && item.gate.risk_dismissed_on != Some(item.today)
                && !item.gate.snooze_until.is_some_and(|until| Utc::now() < until)
        }
        "ended" => resolution.status == Status::Ended && item.view.fresh_break,
        _ => false,
    }
}

struct Chosen {
    account_id: String,
    today: NaiveDate,
    last_active: Option<NaiveDate>,
    kind: NoticeKind,
    reminder: Reminder,
}

fn pick_notice(prepared: &[Prepared]) -> Option<Chosen> {
    let mut best: Option<(u8, i64, Chosen)> = None;
    let now = Utc::now();
    for item in prepared {
        if !item.account.enabled || !item.checked_today {
            continue;
        }
        let Some(resolution) = &item.resolution else {
            continue;
        };
        let Some(kind) = notice_for(resolution, item.today, now, item.quiet, &item.gate) else {
            continue;
        };
        let mins = match kind {
            NoticeKind::AtRisk => item
                .view
                .time_left
                .as_ref()
                .map(|_| minutes_rank(&item.view))
                .unwrap_or(0),
            NoticeKind::Ended => i64::MAX,
        };
        let priority = match kind {
            NoticeKind::AtRisk => 0,
            NoticeKind::Ended => 1,
        };
        let chosen = Chosen {
            account_id: item.account.id.clone(),
            today: item.today,
            last_active: resolution.last_active,
            kind,
            reminder: Reminder {
                account_id: item.account.id.clone(),
                connector_name: item.view.connector_name.clone(),
                label: item.view.label.clone(),
                kind: match kind {
                    NoticeKind::AtRisk => "at_risk",
                    NoticeKind::Ended => "ended",
                }
                .to_string(),
                message: crate::streak::reminder_message(
                    resolution,
                    kind,
                    minutes_from_label(item.view.time_left.as_deref()),
                ),
                service_url: item.view.service_url.clone(),
                streak: resolution.streak,
            },
        };
        let replace = match &best {
            None => true,
            Some((best_priority, best_mins, _)) => {
                priority < *best_priority || (priority == *best_priority && mins < *best_mins)
            }
        };
        if replace {
            best = Some((priority, mins, chosen));
        }
    }
    best.map(|(_, _, chosen)| chosen)
}

fn minutes_rank(view: &AccountView) -> i64 {
    minutes_from_label(view.time_left.as_deref())
}

fn minutes_from_label(label: Option<&str>) -> i64 {
    let Some(label) = label else {
        return 24 * 60;
    };
    if label.contains("under a minute") {
        return 0;
    }
    let number: i64 = label.split_whitespace().next().unwrap_or("0").parse().unwrap_or(0);
    if label.contains("hour") {
        number * 60
    } else {
        number
    }
}

fn persist_notice(app: &AppHandle, chosen: &Chosen) -> Result<(), String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().expect("db");
    let mut gate = db.gate(&chosen.account_id)?;
    mark_shown(&mut gate, chosen.kind, chosen.today, chosen.last_active);
    db.save_gate(&chosen.account_id, &gate)
}

fn present_reminder(app: &AppHandle, reminder: Reminder) {
    {
        let state = app.state::<AppState>();
        *state.active.lock().expect("active") = Some(reminder.clone());
    }
    if let Some(window) = app.get_webview_window("orb") {
        place_orb(&window);
        let _ = window.show();
    }
    let _ = app.emit("reminder", reminder);
}

fn clear_reminder(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        *state.active.lock().expect("active") = None;
    }
    if let Some(window) = app.get_webview_window("orb") {
        let _ = window.hide();
    }
    let _ = app.emit("reminder-clear", ());
}

fn paint_tray(app: &AppHandle, prepared: &[Prepared]) {
    let state = app.state::<AppState>();
    let tray = state.tray.lock().expect("tray").clone();
    let Some(tray) = tray else {
        return;
    };
    let mut rank = 1u8;
    let mut tooltip = if prepared.is_empty() {
        "Add a streak".to_string()
    } else {
        "All streaks are on track".to_string()
    };
    for item in prepared.iter().filter(|item| item.account.enabled) {
        let item_rank = tray_rank(&item.view.status, item.view.fresh_break);
        if item_rank > rank {
            rank = item_rank;
            tooltip = match item.view.status.as_str() {
                "error" => format!("{} couldn't be checked", item.view.label),
                "at_risk" => match &item.view.time_left {
                    Some(left) => format!("{} · {left}", item.view.label),
                    None => format!("{} is still open", item.view.label),
                },
                "ended" if item.view.fresh_break => format!("{} streak ended", item.view.label),
                _ => tooltip.clone(),
            };
        }
    }
    let color = match rank {
        4 => [138, 129, 117],
        3 => [224, 122, 47],
        2 => [180, 35, 24],
        _ => [47, 111, 78],
    };
    let _ = tray.set_icon(Some(circle_icon(color)));
    let _ = tray.set_tooltip(Some(tooltip));
}

fn day_is_stale(app: &AppHandle) -> bool {
    let Ok(prepared) = prepare(app) else {
        return false;
    };
    prepared.iter().any(|item| {
        item.account.enabled && item.view.status != "error" && !item.checked_today && item.view.checked_at.is_some()
    }) || prepared.iter().any(|item| item.account.enabled && item.view.status == "unchecked")
}

fn success_snapshot(resolution: &Resolution, url: String) -> Snapshot {
    Snapshot {
        streak: resolution.streak,
        done_today: resolution.done_today,
        status: resolution.status.as_str().to_string(),
        detail: resolution.detail.clone(),
        error: None,
        checked_at: Some(Utc::now().to_rfc3339()),
        service_url: Some(url),
        last_active: resolution
            .last_active
            .map(|day| day.format("%Y-%m-%d").to_string()),
    }
}

fn error_snapshot(previous: Option<Snapshot>, err: String, fallback_url: String) -> Snapshot {
    let previous = previous.unwrap_or(Snapshot {
        streak: 0,
        done_today: false,
        status: "error".to_string(),
        detail: "Couldn't check".to_string(),
        error: None,
        checked_at: None,
        service_url: None,
        last_active: None,
    });
    Snapshot {
        streak: previous.streak,
        done_today: previous.done_today,
        status: "error".to_string(),
        detail: previous.detail,
        error: Some(err),
        checked_at: Some(Utc::now().to_rfc3339()),
        service_url: previous.service_url.or(Some(fallback_url)),
        last_active: previous.last_active,
    }
}

pub fn account_tz(name: &str) -> Result<Tz, String> {
    parse_tz(name)
}

pub fn circle_icon(rgb: [u8; 3]) -> Image<'static> {
    let size = 44u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let radius = 15.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let alpha = if distance <= radius - 1.0 {
                255
            } else if distance <= radius {
                ((radius - distance) * 255.0) as u8
            } else {
                0
            };
            let index = ((y * size + x) * 4) as usize;
            rgba[index] = rgb[0];
            rgba[index + 1] = rgb[1];
            rgba[index + 2] = rgb[2];
            rgba[index + 3] = alpha;
        }
    }
    Image::new_owned(rgba, size, size)
}

pub fn place_orb(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let Ok(win) = window.outer_size() else {
        return;
    };
    let area = monitor.position();
    let size = monitor.size();
    let scale = monitor.scale_factor();
    let x = area.x + size.width as i32 - win.width as i32 - (20.0 * scale) as i32;
    let y = area.y + size.height as i32 - win.height as i32 - (28.0 * scale) as i32;
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn position_popover(window: &tauri::WebviewWindow, rect: tauri::Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let (x, y, width, height) = match (rect.position, rect.size) {
        (tauri::Position::Physical(pos), tauri::Size::Physical(size)) => {
            (pos.x, pos.y, size.width as i32, size.height as i32)
        }
        (tauri::Position::Logical(pos), tauri::Size::Logical(size)) => (
            (pos.x * scale) as i32,
            (pos.y * scale) as i32,
            (size.width * scale) as i32,
            (size.height * scale) as i32,
        ),
        (tauri::Position::Physical(pos), tauri::Size::Logical(size)) => (
            pos.x,
            pos.y,
            (size.width * scale) as i32,
            (size.height * scale) as i32,
        ),
        (tauri::Position::Logical(pos), tauri::Size::Physical(size)) => (
            (pos.x * scale) as i32,
            (pos.y * scale) as i32,
            size.width as i32,
            size.height as i32,
        ),
    };
    let Ok(win) = window.outer_size() else {
        return;
    };
    let mut left = x + width / 2 - win.width as i32 / 2;
    let top = y + height + 6;
    if let Ok(Some(monitor)) = window.current_monitor() {
        let area = monitor.position();
        let size = monitor.size();
        let min_x = area.x + 8;
        let max_x = area.x + size.width as i32 - win.width as i32 - 8;
        left = left.clamp(min_x, max_x.max(min_x));
    }
    let _ = window.set_position(tauri::PhysicalPosition::new(left, top));
}
