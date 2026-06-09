use tauri::{AppHandle, Runtime, Manager, Emitter};

// ─── Tauri Commands ────────────────────────────────────────────────────

/// Dock layout constants
const DOCK_ITEM_WIDTH: u32 = 48;
const DOCK_GAP: u32 = 6;
const DOCK_DIVIDER_WIDTH: u32 = 13;
const DOCK_PADDING: u32 = 32;
const DOCK_DIVIDER_COUNT: u32 = 2;
const DOCK_MAX_WIDTH: u32 = 1360;
const DOCK_HEIGHT: u32 = 96;
const DOCK_TASKBAR_OFFSET: u32 = 60;
const DOCK_FIXED_ITEMS: u32 = 2; // settings + quit

/// Calculate dock window dimensions from the number of feature items
pub fn calculate_dock_size(item_count: u32) -> (u32, u32) {
    let total_items = item_count + DOCK_FIXED_ITEMS;
    let total_children = total_items + DOCK_DIVIDER_COUNT;
    let gaps_count = if total_children > 1 { total_children - 1 } else { 0 };

    let calculated_width = total_items * DOCK_ITEM_WIDTH
        + gaps_count * DOCK_GAP
        + DOCK_DIVIDER_COUNT * DOCK_DIVIDER_WIDTH
        + DOCK_PADDING;

    let total_width = (calculated_width as u32).min(DOCK_MAX_WIDTH);
    (total_width, DOCK_HEIGHT)
}

/// Position a window at the bottom-center of the primary monitor
pub fn position_window_bottom_center<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let monitor_size = monitor.size();
        let monitor_position = monitor.position();
        let scale_factor = monitor.scale_factor();

        let raw_x = monitor_position.x as f64
            + (monitor_size.width as f64 / scale_factor - width as f64) / 2.0;
        let raw_y = monitor_position.y as f64
            + (monitor_size.height as f64 / scale_factor)
            - height as f64
            - DOCK_TASKBAR_OFFSET as f64;

        let (final_x, final_y) =
            ensure_window_visible(raw_x, raw_y, width as f64, height as f64, &monitor);

        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition {
                x: final_x,
                y: final_y,
            }))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ensure window position is within visible screen bounds
fn ensure_window_visible(
    window_x: f64,
    window_y: f64,
    window_width: f64,
    window_height: f64,
    monitor: &tauri::Monitor,
) -> (f64, f64) {
    let monitor_size = monitor.size();
    let monitor_position = monitor.position();
    let scale_factor = monitor.scale_factor();

    let monitor_left = monitor_position.x as f64;
    let monitor_top = monitor_position.y as f64;
    let monitor_right = monitor_left + (monitor_size.width as f64 / scale_factor);
    let monitor_bottom = monitor_top + (monitor_size.height as f64 / scale_factor);

    let min_visible_width = 100.0;
    let min_visible_height = 30.0;
    let mut final_x = window_x;
    let mut final_y = window_y;

    if window_x + window_width < monitor_left + min_visible_width {
        final_x = monitor_left + 10.0;
    } else if window_x > monitor_right - min_visible_width {
        final_x = monitor_right - window_width - 10.0;
    }

    if window_y + window_height < monitor_top + min_visible_height {
        final_y = monitor_top + 10.0;
    } else if window_y > monitor_bottom - min_visible_height {
        final_y = monitor_bottom - window_height - 10.0;
    }

    if final_x < -10000.0 || final_x > 10000.0 {
        final_x = monitor_left + 10.0;
    }
    if final_y < -10000.0 || final_y > 10000.0 {
        final_y = monitor_top + 10.0;
    }

    (final_x, final_y)
}

#[tauri::command]
pub async fn close_all_child_windows<R: Runtime>(
    app: AppHandle<R>,
    main_window_label: String,
) -> Result<Vec<String>, String> {
    let mut closed_windows = Vec::new();
    let windows = app.webview_windows();

    for (label, window) in windows {
        if label != main_window_label {
            match window.close() {
                Ok(_) => {
                    closed_windows.push(label);
                }
                Err(e) => {
                    eprintln!("关闭窗口 {label} 失败: {e}");
                }
            }
        }
    }

    Ok(closed_windows)
}

#[tauri::command]
pub async fn get_all_child_windows<R: Runtime>(
    app: AppHandle<R>,
    main_window_label: String,
) -> Result<Vec<String>, String> {
    let windows = app.webview_windows();
    let child_windows: Vec<String> = windows
        .keys()
        .filter(|&label| label != &main_window_label)
        .cloned()
        .collect();

    Ok(child_windows)
}

#[tauri::command]
pub async fn force_close_main_window<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    std::process::exit(0);
}

#[tauri::command]
pub async fn show_main_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_main_window_size_for_dock<R: Runtime>(
    app: AppHandle<R>,
    item_count: u32,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let (total_width, total_height) = calculate_dock_size(item_count);

        println!(
            "[set_main_window_size_for_dock] item_count={item_count}, final_width={total_width}"
        );

        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: total_width as f64,
                height: total_height as f64,
            }))
            .map_err(|e| e.to_string())?;

        position_window_bottom_center(&window, total_width, total_height)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_main_window_opacity<R: Runtime>(_app: AppHandle<R>, _opacity: f64) -> Result<(), String> {
    // TODO: Tauri v2 opacity API change
    Ok(())
}

#[tauri::command]
pub async fn get_main_window_opacity<R: Runtime>(_app: AppHandle<R>) -> Result<f64, String> {
    // TODO: Tauri v2 opacity API change
    Ok(1.0)
}

#[tauri::command]
pub async fn set_main_window_always_on_top<R: Runtime>(
    app: AppHandle<R>,
    always_on_top: bool,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_always_on_top(always_on_top)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_function_window<R: Runtime>(
    app: AppHandle<R>,
    page_name: String,
) -> Result<(), String> {
    use tauri::WebviewWindowBuilder;

    let title = match page_name.as_str() {
        "transfer" => "💸 批量转账",
        "balance" => "💰 余额查询",
        "monitor" => "👁️ 链上监控",
        _ => "❓ 未知功能",
    };

    let existing_windows = app.webview_windows();
    let mut window_count = 1;

    let window_label = loop {
        let candidate_label = format!("{page_name}{window_count}");
        if !existing_windows.contains_key(&candidate_label) {
            break candidate_label;
        }
        window_count += 1;
        if window_count > 100 {
            return Err("无法找到可用的窗口标签，已达到最大窗口数量限制".to_string());
        }
    };

    let window_url = format!("/#/{page_name}?count={window_count}");

    let window_title = if window_count > 1 {
        format!("WalletsTool - {title} [{window_count}]")
    } else {
        format!("WalletsTool - {title}")
    };

    let webview = WebviewWindowBuilder::new(
        &app,
        &window_label,
        tauri::WebviewUrl::App(window_url.into()),
    )
    .title(&window_title)
    .inner_size(1350.0, 900.0)
    .resizable(true)
    .center()
    .decorations(false)
    .visible(false)
    .skip_taskbar(false)
    .build()
    .map_err(|e| e.to_string())?;

    webview.show().map_err(|e| e.to_string())?;

    Ok(())
}

/// Setup the initial main window size, position, and display.
/// Called once during app startup.
pub fn setup_main_window<R: Runtime>(app: &tauri::App<R>) {
    let dock_item_count = 6u32;
    let (total_width, total_height) = calculate_dock_size(dock_item_count);

    if let Some(window) = app.get_webview_window("main") {
        println!("[setup] Setting initial window size: {total_width}x{total_height}");

        if let Err(e) = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: total_width as f64,
            height: total_height as f64,
        })) {
            eprintln!("设置主窗口初始大小失败: {e}");
        }

        if let Err(e) = position_window_bottom_center(&window, total_width, total_height) {
            eprintln!("设置主窗口初始位置失败: {e}");
        }

        if let Err(e) = window.show() {
            eprintln!("显示主窗口失败: {e}");
        } else {
            println!("[setup] Window shown successfully at correct position");
        }

        if let Err(e) = window.set_focus() {
            eprintln!("设置主窗口焦点失败: {e}");
        }

        #[cfg(target_os = "windows")]
        if let Err(e) = window.set_shadow(false) {
            eprintln!("禁用主窗口阴影失败: {e}");
        }
    }
}
