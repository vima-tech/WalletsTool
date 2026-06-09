#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod wallets_tool;
mod plugins;
mod database;

use tauri::{WindowEvent, Manager, Emitter, tray::TrayIconBuilder, menu::{MenuBuilder, MenuItemBuilder}};
use wallets_tool::airdrop::scheduler::TaskScheduler;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // 启动安全保护
    wallets_tool::security::enable_protection();

    // 初始化公开数据库（链配置、RPC节点等）
    if let Err(err) = database::init_public_database().await {
        eprintln!("公开数据库初始化失败: {err:?}");
        return;
    }

    // 使用公开数据库连接池
    let sqlite_pool = database::DualDatabaseManager::public_pool();
    println!("Initializing WalletManagerService...");
    let wallet_manager_service =
        wallets_tool::wallet_manager::service::WalletManagerService::new(sqlite_pool.clone());
    
    let chain_service = database::chain_service::ChainService::new();
    
    // Initialize task scheduler for browser automation
    let task_scheduler = Arc::new(TaskScheduler::new(sqlite_pool.clone()));
    let scheduler_for_setup = task_scheduler.clone();
    
    let updater_pubkey = option_env!("WALLETSTOOL_UPDATER_PUBKEY").unwrap_or("").trim();
    let updater_plugin = if updater_pubkey.is_empty() {
        tauri_plugin_updater::Builder::new().build()
    } else {
        tauri_plugin_updater::Builder::new()
            .pubkey(updater_pubkey)
            .build()
    };

    // Bring window_manager functions into scope for the invoke_handler macro
    use wallets_tool::window_manager::{
        close_all_child_windows, get_all_child_windows, force_close_main_window,
        show_main_window, set_main_window_size_for_dock, set_main_window_opacity,
        get_main_window_opacity, set_main_window_always_on_top, open_function_window,
    };
    
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(updater_plugin)
        .manage(sqlite_pool)
        .manage(wallet_manager_service)
        .manage(chain_service)
        .manage(task_scheduler)
        .setup(move |app| {
            // Start the task scheduler
            let scheduler = scheduler_for_setup;
            tauri::async_runtime::spawn(async move {
                scheduler.start().await;
            });

            // 主窗口初始大小和位置（使用共享辅助函数，消除重复计算）
            wallets_tool::window_manager::setup_main_window(app);

            // 构建托盘菜单
            let show_main = MenuItemBuilder::new("显示主窗口").id("show_main").build(app)?;
            let separator1 = tauri::menu::PredefinedMenuItem::separator(app)?;
            let batch_transfer = MenuItemBuilder::new("批量转账").id("transfer").build(app)?;
            let balance_query = MenuItemBuilder::new("余额查询").id("balance").build(app)?;
            let separator2 = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit = MenuItemBuilder::new("退出程序").id("quit").build(app)?;
            
            let menu = MenuBuilder::new(app)
                .item(&show_main)
                .item(&separator1)
                .item(&batch_transfer)
                .item(&balance_query)
                .item(&separator2)
                .item(&quit)
                .build()?;
            
            // 创建托盘图标
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "show_main" => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = show_main_window(app_handle).await {
                                    eprintln!("显示主窗口失败: {e}");
                                }
                            });
                        }
                        "transfer" => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = open_function_window(app_handle, "transfer".to_string()).await {
                                    eprintln!("打开批量转账窗口失败: {e}");
                                }
                            });
                        }
                        "balance" => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = open_function_window(app_handle, "balance".to_string()).await {
                                    eprintln!("打开余额查询窗口失败: {e}");
                                }
                            });
                        }
                        "quit" => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                // 先显示主窗口
                                if let Err(e) = show_main_window(app_handle.clone()).await {
                                    eprintln!("显示主窗口失败: {e}");
                                }

                                // 发送退出确认事件到前端
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    if let Err(e) = window.emit("tray-quit-requested", ()) {
                                        eprintln!("发送托盘退出事件失败: {e}");
                                    }
                                }
                            });
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    match event {
                        tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..  
                        } => {
                            let app_handle = tray.app_handle().clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = show_main_window(app_handle).await {
                                    eprintln!("左键点击托盘显示主窗口失败: {e}");
                                }
                            });
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let window_label = window.label().to_string();

                if window_label == "main" {
                    api.prevent_close();
                    
                    if let Err(e) = window.show() {
                        eprintln!("显示主窗口失败: {e}");
                    }
                    if let Err(e) = window.set_focus() {
                        eprintln!("设置主窗口焦点失败: {e}");
                    }
                    
                    if let Err(e) = window.set_always_on_top(true) {
                        eprintln!("设置窗口置顶失败: {e}");
                    }
                    
                    if let Err(e) = window.emit("main-window-close-requested", ()) {
                        eprintln!("发送关闭事件失败: {e}");
                    }
                    
                    let window_clone = window.clone();
                    
                    tokio::spawn(async move {
                        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                        if let Err(e) = window_clone.set_always_on_top(false) {
                            eprintln!("恢复窗口正常状态失败: {e}");
                        }
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // ─── Chain Config ───
            wallets_tool::ecosystems::ethereum::chain_config::get_chain_list,
            wallets_tool::ecosystems::ethereum::chain_config::get_coin_list,
            wallets_tool::ecosystems::ethereum::chain_config::add_coin,
            wallets_tool::ecosystems::ethereum::chain_config::remove_coin,
            wallets_tool::ecosystems::ethereum::chain_config::update_coin,
            wallets_tool::ecosystems::ethereum::chain_config::update_chain_pic_urls,
            wallets_tool::ecosystems::ethereum::chain_config::update_token_abi,
            wallets_tool::ecosystems::ethereum::chain_config::add_chain,
            wallets_tool::ecosystems::ethereum::chain_config::update_chain,
            wallets_tool::ecosystems::ethereum::chain_config::remove_chain,
            wallets_tool::ecosystems::ethereum::chain_config::get_chain_detail,
            // ─── Utils ───
            wallets_tool::utils::download_file,
            wallets_tool::utils::save_chain_icon,
            wallets_tool::utils::get_chain_icon,
            wallets_tool::utils::read_resource_file,
            wallets_tool::utils::save_file,
            wallets_tool::utils::get_temp_dir,
            wallets_tool::utils::open_file_directory,
            // ─── Update ───
            wallets_tool::update::check_github_release_update,
            wallets_tool::update::check_update,
            wallets_tool::update::download_and_install_update,
            wallets_tool::update::download_and_install_from_url,
            wallets_tool::update::download_update_only,
            // ─── FS Extra ───
            plugins::fs_extra::exists,
            plugins::fs_extra::open_file,
            // ─── Balance Query ───
            wallets_tool::ecosystems::ethereum::simple_balance_query::query_balances_simple,
            wallets_tool::ecosystems::ethereum::simple_balance_query::query_balances_with_updates,
            wallets_tool::ecosystems::ethereum::simple_balance_query::stop_balance_query,
            wallets_tool::ecosystems::ethereum::simple_balance_query::reset_balance_query_stop,
            // ─── Window Management (extracted to window_manager module) ───
            close_all_child_windows,
            get_all_child_windows,
            force_close_main_window,
            show_main_window,
            set_main_window_size_for_dock,
            set_main_window_opacity,
            get_main_window_opacity,
            set_main_window_always_on_top,
            open_function_window,
            // ─── Database ───
            database::reload_database,
            database::check_database_schema,
            database::export_database_to_init_sql,
            database::commands::get_dual_database_status,
            database::commands::init_public_db,
            database::commands::init_secure_db,
            database::commands::unlock_secure_db,
            database::commands::lock_secure_db,
            database::commands::is_secure_db_initialized,
            database::commands::is_public_db_ready,
            database::commands::is_secure_db_unlocked,
            database::commands::is_wallet_manager_ready,
            // ─── EVM Transfer ───
            wallets_tool::transfer::base_coin_transfer,
            wallets_tool::transfer::base_coin_transfer_fast,
            wallets_tool::transfer::check_transactions_status_batch,
            wallets_tool::transfer::check_transaction_status,
            wallets_tool::transfer::query_balance,
            wallets_tool::transfer::check_wallet_recent_transfers,
            wallets_tool::transfer::stop_transfer,
            wallets_tool::transfer::reset_transfer_stop,
            // ─── Solana Transfer ───
            wallets_tool::ecosystems::solana::transfer::sol_transfer,
            wallets_tool::ecosystems::solana::transfer::sol_token_transfer,
            wallets_tool::ecosystems::solana::transfer::sol_transfer_fast,
            wallets_tool::ecosystems::solana::transfer::sol_token_transfer_fast,
            wallets_tool::ecosystems::solana::transfer::sol_check_recent_transfers,
            wallets_tool::ecosystems::solana::transfer::sol_check_transactions_status_batch,
            wallets_tool::ecosystems::solana::transfer::sol_query_balances_with_updates,
            wallets_tool::ecosystems::solana::provider::test_solana_rpc_connection,
            // ─── Token Transfer ───
            wallets_tool::token_transfer::token_transfer,
            wallets_tool::token_transfer::token_transfer_fast,
            wallets_tool::token_transfer::query_token_balance,
            wallets_tool::token_transfer::get_token_info,
            // ─── Provider ───
            wallets_tool::provider::get_chain_gas_price,
            wallets_tool::provider::test_rpc_url,
            wallets_tool::provider::get_multiple_gas_prices,
            // ─── RPC Management ───
            wallets_tool::ecosystems::ethereum::rpc_management::get_rpc_providers,
            wallets_tool::ecosystems::ethereum::rpc_management::add_rpc_provider,
            wallets_tool::ecosystems::ethereum::rpc_management::update_rpc_provider,
            wallets_tool::ecosystems::ethereum::rpc_management::delete_rpc_provider,
            wallets_tool::ecosystems::ethereum::rpc_management::test_rpc_connection,
            // ─── Proxy Management ───
            wallets_tool::ecosystems::ethereum::proxy_commands::set_proxy_window_id,
            wallets_tool::ecosystems::ethereum::proxy_commands::save_proxy_config,
            wallets_tool::ecosystems::ethereum::proxy_commands::save_proxy_config_for_window,
            wallets_tool::ecosystems::ethereum::proxy_commands::get_proxy_config,
            wallets_tool::ecosystems::ethereum::proxy_commands::get_proxy_config_for_window,
            wallets_tool::ecosystems::ethereum::proxy_commands::test_proxy_connection,
            wallets_tool::ecosystems::ethereum::proxy_commands::get_proxy_stats,
            wallets_tool::ecosystems::ethereum::proxy_commands::get_proxy_stats_for_window,
            wallets_tool::ecosystems::ethereum::proxy_commands::clear_proxy_config_for_window,
            // ─── Wallet Manager ───
            wallets_tool::wallet_manager::commands::init_wallet_manager_tables,
            wallets_tool::wallet_manager::commands::is_wallet_manager_initialized,
            wallets_tool::wallet_manager::commands::init_encrypted_db,
            wallets_tool::wallet_manager::commands::unlock_encrypted_db,
            wallets_tool::wallet_manager::commands::is_password_set,
            wallets_tool::wallet_manager::commands::init_password,
            wallets_tool::wallet_manager::commands::verify_password,
            wallets_tool::wallet_manager::commands::get_wallet_transport_public_key,
            wallets_tool::wallet_manager::commands::register_wallet_transport_key,
            wallets_tool::wallet_manager::commands::change_password,
            wallets_tool::wallet_manager::commands::create_group,
            wallets_tool::wallet_manager::commands::get_groups,
            wallets_tool::wallet_manager::commands::update_group,
            wallets_tool::wallet_manager::commands::delete_group,
            wallets_tool::wallet_manager::commands::create_wallet,
            wallets_tool::wallet_manager::commands::create_wallets,
            wallets_tool::wallet_manager::commands::get_wallets,
            wallets_tool::wallet_manager::commands::get_wallet_secrets,
            wallets_tool::wallet_manager::commands::export_wallets,
            wallets_tool::wallet_manager::commands::update_wallet,
            wallets_tool::wallet_manager::commands::delete_wallet,
            // ─── Watch Address ───
            wallets_tool::wallet_manager::commands::get_watch_addresses,
            wallets_tool::wallet_manager::commands::create_watch_address,
            wallets_tool::wallet_manager::commands::create_watch_addresses,
            wallets_tool::wallet_manager::commands::update_watch_address,
            wallets_tool::wallet_manager::commands::delete_watch_address,
            wallets_tool::wallet_manager::commands::export_watch_addresses,
            // ─── Encrypted Cloud Backup ───
            wallets_tool::wallet_manager::commands::create_encrypted_backup,
            wallets_tool::wallet_manager::commands::restore_encrypted_backup,
            wallets_tool::wallet_manager::commands::save_backup_to_file,
            wallets_tool::wallet_manager::commands::load_backup_from_file,
            // ─── Browser Automation (Airdrop) ───
            wallets_tool::airdrop::commands::init_browser_automation_tables,
            wallets_tool::airdrop::commands::get_airdrop_wallets,
            wallets_tool::airdrop::commands::create_airdrop_wallet,
            wallets_tool::airdrop::commands::update_airdrop_wallet,
            wallets_tool::airdrop::commands::delete_airdrop_wallet,
            wallets_tool::airdrop::commands::import_airdrop_wallets,
            wallets_tool::airdrop::commands::get_wallet_private_key,
            wallets_tool::airdrop::commands::get_browser_profiles,
            wallets_tool::airdrop::commands::create_browser_profile,
            wallets_tool::airdrop::commands::update_browser_profile,
            wallets_tool::airdrop::commands::delete_browser_profile,
            wallets_tool::airdrop::commands::batch_generate_profiles,
            wallets_tool::airdrop::commands::get_automation_scripts,
            wallets_tool::airdrop::commands::create_automation_script,
            wallets_tool::airdrop::commands::update_automation_script,
            wallets_tool::airdrop::commands::delete_automation_script,
            wallets_tool::airdrop::commands::get_automation_tasks,
            wallets_tool::airdrop::commands::create_automation_task,
            wallets_tool::airdrop::commands::update_automation_task,
            wallets_tool::airdrop::commands::delete_automation_task,
            wallets_tool::airdrop::commands::toggle_task_status,
            wallets_tool::airdrop::commands::get_task_executions,
            wallets_tool::airdrop::commands::delete_task_execution,
            wallets_tool::airdrop::commands::get_task_execution_stats,
            wallets_tool::airdrop::commands::run_task_now,
            // ─── Execution ───
            wallets_tool::airdrop::commands::create_execution,
            wallets_tool::airdrop::commands::start_execution,
            wallets_tool::airdrop::commands::cancel_execution,
            wallets_tool::airdrop::commands::get_execution,
            wallets_tool::airdrop::commands::simulate_execution,
            // ─── Browser Extensions ───
            wallets_tool::airdrop::commands::get_browser_extensions,
            wallets_tool::airdrop::commands::create_browser_extension,
            wallets_tool::airdrop::commands::update_browser_extension,
            wallets_tool::airdrop::commands::delete_browser_extension,
            wallets_tool::airdrop::commands::toggle_browser_extension,
            wallets_tool::airdrop::commands::scan_extension_folder,
            wallets_tool::airdrop::commands::import_extension_from_folder,
            // ─── Playwright ───
            wallets_tool::playwright::execute_playwright_script,
            wallets_tool::playwright::executor::playwright_create_session,
            wallets_tool::playwright::executor::playwright_start_execution,
            wallets_tool::playwright::executor::playwright_get_execution_status,
            wallets_tool::playwright::executor::playwright_cancel_execution,
            wallets_tool::playwright::executor::playwright_cleanup_session,
            wallets_tool::playwright::recorder::playwright_start_recording,
            wallets_tool::playwright::recorder::playwright_stop_recording,
            wallets_tool::playwright::recorder::playwright_get_recording_session,
            wallets_tool::playwright::recorder::check_cli_tools,
            wallets_tool::playwright::recorder::install_node_environment,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
