mod commands;
mod connectors;
mod scheduler;
mod secrets;
mod store;
mod streak;

use std::time::{Duration, Instant};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

use scheduler::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::dashboard,
            commands::catalog,
            commands::active_reminder,
            commands::refresh_now,
            commands::test_connector,
            commands::save_account,
            commands::delete_account,
            commands::set_enabled,
            commands::get_settings,
            commands::save_settings,
            commands::snooze,
            commands::dismiss_reminder,
            commands::open_service,
            commands::show_settings,
            commands::quit,
        ])
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .expect("the app data directory is available");
            let store = store::Store::open(&dir.join("streaks.db")).expect("the streak database opens");
            let launch_at_login = store.settings().map(|settings| settings.launch_at_login).unwrap_or(true);
            let http = reqwest::Client::builder()
                .user_agent("streak-orb")
                .timeout(Duration::from_secs(12))
                .build()
                .expect("the http client builds");
            app.manage(AppState {
                db: std::sync::Mutex::new(store),
                http,
                tray: std::sync::Mutex::new(None),
                active: std::sync::Mutex::new(None),
                last_check: std::sync::Mutex::new(None),
                checking: std::sync::Mutex::new(false),
                popover_grace: std::sync::Mutex::new(Instant::now()),
            });

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let check = MenuItem::with_id(app, "check", "Check now", true, None::<&str>)?;
            let streaks = MenuItem::with_id(app, "streaks", "Streaks…", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Streak Orb", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&check, &streaks, &separator, &quit])?;
            let icon = scheduler::circle_icon([47, 111, 78]);
            let tray = TrayIconBuilder::with_id("streak-orb")
                .icon(icon)
                .tooltip("Streak Orb")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "check" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            scheduler::run_checks(&app).await;
                        });
                    }
                    "streaks" => commands::show_settings(app.clone()),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), rect);
                    }
                })
                .build(app)?;
            *handle.state::<AppState>().tray.lock().expect("tray") = Some(tray);

            if let Some(popover) = handle.get_webview_window("popover") {
                let blur_handle = handle.clone();
                popover.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let grace = *blur_handle
                            .state::<AppState>()
                            .popover_grace
                            .lock()
                            .expect("grace");
                        if Instant::now() < grace {
                            return;
                        }
                        if let Some(window) = blur_handle.get_webview_window("popover") {
                            let _ = window.hide();
                        }
                    }
                });
            }
            if let Some(settings) = handle.get_webview_window("settings") {
                let settings_handle = handle.clone();
                settings.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(window) = settings_handle.get_webview_window("settings") {
                            let _ = window.hide();
                        }
                    }
                });
            }

            commands::apply_launch_login(&handle, launch_at_login);
            scheduler::spawn(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("streak orb runs");
}

fn toggle_popover(app: &tauri::AppHandle, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window("popover") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    scheduler::position_popover(&window, rect);
    *app.state::<AppState>()
        .popover_grace
        .lock()
        .expect("grace") = Instant::now() + Duration::from_millis(280);
    let _ = window.show();
    let _ = window.set_focus();
}
