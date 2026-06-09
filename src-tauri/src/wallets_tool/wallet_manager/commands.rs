use tauri::State;
use crate::wallets_tool::wallet_manager::service::WalletManagerService;
use crate::database::get_database_pool;
use super::models::*;

/// 初始化钱包管理器的表结构（使用最新的数据库连接池）
#[tauri::command]
pub async fn init_wallet_manager_tables() -> Result<(), String> {
    // 直接从全局获取最新的数据库连接池，避免使用可能过时的 State
    let pool = get_database_pool();
    let service = WalletManagerService::new(pool.clone());
    service.init_tables().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_wallet_manager_initialized() -> Result<bool, String> {
    // 首先检查新架构的secure.db是否已初始化
    if crate::database::is_secure_database_initialized() {
        return Ok(true);
    }

    // 后备：检查app_config表（兼容旧架构）
    let pool = get_database_pool();
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM app_config WHERE key = 'master_verifier'"
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(0);

    Ok(count > 0)
}

/// 初始化安全数据库（委托给新架构的secure_db）
#[tauri::command]
pub async fn init_encrypted_db(password: String) -> Result<(), String> {
    crate::database::secure_init::init_secure_database(&password).await.map_err(|e| e.to_string())
}

/// 解锁安全数据库
#[tauri::command]
pub async fn unlock_encrypted_db(password: String) -> Result<(), String> {
    crate::database::secure_init::unlock_secure_database(&password).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_password_set(state: State<'_, WalletManagerService>) -> Result<bool, String> {
    state.is_password_set().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn init_password(state: State<'_, WalletManagerService>, request: InitPasswordRequest) -> Result<(), String> {
    let password = if let Some(enc) = request.encrypted_password_b64.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        state.open_rsa_oaep_b64(enc).map_err(|e| e.to_string())?
    } else {
        request.password.unwrap_or_default()
    };
    state.init_password(password.trim()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn verify_password(state: State<'_, WalletManagerService>, request: VerifyPasswordRequest) -> Result<bool, String> {
    let password = if let Some(enc) = request.encrypted_password_b64.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        state.open_rsa_oaep_b64(enc).map_err(|e| e.to_string())?
    } else {
        request.password.unwrap_or_default()
    };
    state.unlock(password.trim()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn change_password(state: State<'_, WalletManagerService>, request: ChangePasswordRequest) -> Result<(), String> {
    state.change_password(&request.old_password, &request.new_password).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_group(state: State<'_, WalletManagerService>, request: CreateGroupRequest) -> Result<i64, String> {
    state.create_group(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_groups(state: State<'_, WalletManagerService>) -> Result<Vec<WalletGroup>, String> {
    state.get_groups().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_group(state: State<'_, WalletManagerService>, request: UpdateGroupRequest) -> Result<(), String> {
    state.update_group(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_group(state: State<'_, WalletManagerService>, id: i64) -> Result<(), String> {
    state.delete_group(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_wallet(state: State<'_, WalletManagerService>, request: CreateWalletRequest) -> Result<i64, String> {
    state.create_wallet(request).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_wallets(
    state: State<'_, WalletManagerService>,
    request: CreateWalletsRequest,
    address: Option<String>
) -> Result<CreateWalletsResult, String> {
    state.create_wallets(request, address).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_wallets(
    state: State<'_, WalletManagerService>, 
    group_id: Option<i64>, 
    chain_type: Option<String>,
    password: Option<String>
) -> Result<Vec<WalletInfo>, String> {
    state.get_wallets(group_id, chain_type, password).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_wallet(state: State<'_, WalletManagerService>, request: UpdateWalletRequest) -> Result<(), String> {
    state.update_wallet(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_wallet(state: State<'_, WalletManagerService>, id: i64) -> Result<(), String> {
    state.delete_wallet(id).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_wallet_secrets(
    state: State<'_, WalletManagerService>,
    id: i64,
    password: Option<String>,
    transport_token: Option<String>,
) -> Result<WalletSecrets, String> {
    state.get_wallet_secrets(id, password.as_deref(), transport_token.as_deref()).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_wallets(
    state: State<'_, WalletManagerService>,
    ids: Vec<i64>,
    password: String,
) -> Result<Vec<WalletExportData>, String> {
    state.export_wallets(&ids, &password).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_wallet_transport_public_key(state: State<'_, WalletManagerService>) -> Result<String, String> {
    state.get_transport_public_key_pem().map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn register_wallet_transport_key(
    state: State<'_, WalletManagerService>,
    encrypted_key_b64: String,
) -> Result<String, String> {
    state.register_transport_key(&encrypted_key_b64).map_err(|e| e.to_string())
}

// ==================== Watch Address Commands ====================

#[tauri::command(rename_all = "snake_case")]
pub async fn get_watch_addresses(
    state: State<'_, WalletManagerService>,
    group_id: Option<i64>,
    chain_type: Option<String>,
) -> Result<Vec<WatchAddressInfo>, String> {
    state.get_watch_addresses(group_id, chain_type).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_watch_address(
    state: State<'_, WalletManagerService>,
    request: CreateWatchAddressRequest,
) -> Result<i64, String> {
    state.create_watch_address(request).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_watch_addresses(
    state: State<'_, WalletManagerService>,
    request: CreateWatchAddressesRequest,
) -> Result<u32, String> {
    state.create_watch_addresses(request).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_watch_address(
    state: State<'_, WalletManagerService>,
    request: UpdateWatchAddressRequest,
) -> Result<(), String> {
    state.update_watch_address(request).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_watch_address(state: State<'_, WalletManagerService>, id: i64) -> Result<(), String> {
    state.delete_watch_address(id).await.map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn export_watch_addresses(
    state: State<'_, WalletManagerService>,
    ids: Vec<i64>,
) -> Result<Vec<WatchAddressExportData>, String> {
    state.export_watch_addresses(&ids).await.map_err(|e| e.to_string())
}

// ==================== Encrypted Cloud Backup Commands ====================

/// 创建加密备份
#[tauri::command(rename_all = "snake_case")]
pub async fn create_encrypted_backup(
    state: State<'_, WalletManagerService>,
    request: CreateBackupRequest,
) -> Result<EncryptedBackup, String> {
    request.validate_password()?;
    state.create_encrypted_backup(request).await.map_err(|e| e.to_string())
}

/// 恢复加密备份
#[tauri::command(rename_all = "snake_case")]
pub async fn restore_encrypted_backup(
    state: State<'_, WalletManagerService>,
    request: RestoreBackupRequest,
) -> Result<RestoreResult, String> {
    state.restore_encrypted_backup(request).await.map_err(|e| e.to_string())
}

/// 将备份保存到文件
#[tauri::command(rename_all = "snake_case")]
pub async fn save_backup_to_file(
    backup: EncryptedBackup,
    file_path: String,
) -> Result<(), String> {
    let json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    tokio::fs::write(&file_path, json).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// 从文件加载备份
#[tauri::command(rename_all = "snake_case")]
pub async fn load_backup_from_file(
    file_path: String,
) -> Result<EncryptedBackup, String> {
    let json = tokio::fs::read_to_string(&file_path).await.map_err(|e| e.to_string())?;
    let backup: EncryptedBackup = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(backup)
}
