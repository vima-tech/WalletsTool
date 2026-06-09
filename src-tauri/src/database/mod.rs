pub mod models;
pub mod chain_service;
pub mod rpc_service;

pub mod dual_database;
pub mod encryption;
pub mod public_init;
pub mod secure_init;
pub mod commands;

#[allow(unused_imports)]
pub use dual_database::{
    DualDatabaseManager, 
    DatabaseStatus, 
    SecureDbState,
    get_wallet_database_pool,
    PUBLIC_DB_PATH,
    SECURE_DB_PATH,
    LEGACY_DB_PATH,
};

#[allow(unused_imports)]
pub use public_init::init_public_database;
#[allow(unused_imports)]
pub use secure_init::{
    init_secure_database,
    unlock_secure_database,
    lock_secure_database,
    change_secure_password,
    is_secure_database_initialized,
};

#[allow(unused_imports)]
pub use commands::*;

#[allow(dead_code)]
static PUBLIC_INIT_SQL_CONTENT: &str = include_str!("../../data/public_init.sql");

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions, Row};
use anyhow::Result;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use serde::{Deserialize, Serialize};
use std::path::Path;
/// Package.json 配置结构
#[derive(Debug, Deserialize, Serialize)]
struct PackageJson {
    config: Option<Config>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    database: Option<DatabaseConfig>,
}

/// 数据库配置
#[derive(Debug, Deserialize, Serialize)]
struct DatabaseConfig {
    #[serde(rename = "forceInit")]
    force_init: Option<bool>,
    #[serde(rename = "enableDebugLog")]
    enable_debug_log: Option<bool>,
    #[serde(rename = "initSqlPath")]
    init_sql_path: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            force_init: Some(false),
            enable_debug_log: Some(true),
            init_sql_path: Some("data/public_init.sql".to_string()),
        }
    }
}

/// 读取package.json中的数据库配置
fn load_database_config() -> DatabaseConfig {
    let package_json_paths = vec!["package.json", "../package.json"];
    
    for path in package_json_paths {
        if Path::new(path).exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    match serde_json::from_str::<PackageJson>(&content) {
                        Ok(package_json) => {
                            if let Some(config) = package_json.config {
                                if let Some(db_config) = config.database {
                                    return DatabaseConfig {
                                        force_init: db_config.force_init,
                                        enable_debug_log: db_config.enable_debug_log,
                                        init_sql_path: db_config.init_sql_path,
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("解析 {path} 失败: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("读取 {path} 失败: {e}");
                }
            }
        }
    }
    
    // 如果找不到配置文件或配置项，返回默认配置
    DatabaseConfig::default()
}

/// 数据库管理器
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    /// 创建新的数据库管理器实例
    #[allow(dead_code)]
    pub async fn new(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true);
        
        let pool = SqlitePool::connect_with(options).await?;
        
        // 启用外键约束
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
        
        let manager = Self { pool };
        
        Ok(manager)
    }
    
    /// 获取数据库连接池
    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 创建加密的数据库管理器实例
    pub async fn new_encrypted(database_url: &str, db_password: &str) -> Result<Self> {
        let db_password_for_hook = db_password.to_string();
        
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true);
        
        let pool = sqlx::pool::PoolOptions::<sqlx::Sqlite>::new()
            .max_connections(1)
            .min_connections(1)
            .after_connect(move |conn, _meta| {
                let password = db_password_for_hook.clone();
                Box::pin(async move {
                    sqlx::query(&format!("PRAGMA key = '{}'", password.replace("'", "''")))
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        
        let manager = Self { pool };
        Ok(manager)
    }
}

/// 辅助函数：删除数据库文件及其关联的 WAL/SHM 文件
async fn delete_db_files(db_path: &str, enable_debug: bool) -> Result<(), String> {
    let db_path = Path::new(db_path);

    // 删除 WAL 文件（如果存在）
    let wal_path = format!("{}-wal", db_path.display());
    let wal_path = Path::new(&wal_path);
    if wal_path.exists() {
        if let Err(e) = tokio::fs::remove_file(wal_path).await {
            if enable_debug {
                eprintln!("警告：删除 WAL 文件失败: {}", e);
            }
        } else if enable_debug {
            println!("已删除 WAL 文件: {}", wal_path.display());
        }
    }

    // 删除 SHM 文件（如果存在）
    let shm_path = format!("{}-shm", db_path.display());
    let shm_path = Path::new(&shm_path);
    if shm_path.exists() {
        if let Err(e) = tokio::fs::remove_file(shm_path).await {
            if enable_debug {
                eprintln!("警告：删除 SHM 文件失败: {}", e);
            }
        } else if enable_debug {
            println!("已删除 SHM 文件: {}", shm_path.display());
        }
    }

    // 删除主数据库文件
    if db_path.exists() {
        // 多次尝试删除（处理 Windows 文件锁延迟释放）
        let max_retries = 5;
        for attempt in 1..=max_retries {
            match tokio::fs::remove_file(db_path).await {
                Ok(()) => {
                    if enable_debug {
                        println!("已删除数据库文件: {}", db_path.display());
                    }
                    return Ok(());
                }
                Err(e) if attempt < max_retries && e.raw_os_error() == Some(32) => {
                    // Windows 文件锁定错误，等待后重试
                    if enable_debug {
                        println!("文件被锁定，等待重试 ({}/{})...", attempt, max_retries);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
                Err(e) => {
                    return Err(format!("删除数据库文件失败 ({}): {}", db_path.display(), e));
                }
            }
        }
    }

    Ok(())
}

/// 恢复出厂设置（删除数据库文件并重新初始化）
#[tauri::command]
pub async fn reload_database() -> Result<String, String> {
    let config = load_database_config();
    let enable_debug = config.enable_debug_log.unwrap_or(false);

    if enable_debug {
        println!("开始恢复出厂设置...");
    }

    let public_db_path = "data/public.db";
    let secure_db_path = "data/secure.db";
    let legacy_db_path = "data/wallets_tool.db";

    // 首先关闭新的双数据库连接
    DualDatabaseManager::force_disconnect_all().await;

    // 同时关闭旧的全局 DATABASE_POOL（如果存在）
    if let Some(lock) = DATABASE_POOL.get() {
        let pool = lock.read().unwrap().clone();
        pool.close().await;
    }

    // 等待确保所有文件锁释放
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 删除 WAL/SHM 和主数据库文件
    delete_db_files(public_db_path, enable_debug).await?;
    // 等待确保文件锁释放
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    delete_db_files(secure_db_path, enable_debug).await?;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    delete_db_files(legacy_db_path, enable_debug).await?;
    // 等待确保文件锁释放
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // 重新初始化公开数据库
    init_public_database().await.map_err(|e| e.to_string())?;

    if enable_debug {
        println!("公开数据库初始化完成");
    }

    Ok("恢复出厂设置完成".to_string())
}


/// 全局数据库连接池（使用 RwLock 支持运行时更新）
static DATABASE_POOL: OnceLock<RwLock<SqlitePool>> = OnceLock::new();

/// 更新全局数据库连接池
fn update_global_pool(pool: SqlitePool) {
    if let Some(lock) = DATABASE_POOL.get() {
        let mut guard = lock.write().unwrap();
        *guard = pool;
    } else {
        let _ = DATABASE_POOL.set(RwLock::new(pool));
    }
}

/// 获取全局数据库连接池的克隆
/// 在双库模式下，返回公开数据库连接池
pub fn get_database_pool() -> SqlitePool {
    if DualDatabaseManager::public_pool_ready() {
        return DualDatabaseManager::public_pool();
    }
    DATABASE_POOL
        .get()
        .expect("Database pool not initialized")
        .read()
        .unwrap()
        .clone()
}

/// 初始化测试用的全局数据库连接池（仅用于测试）
#[cfg(test)]
pub fn init_test_pool(pool: SqlitePool) {
    // 使用 update_global_pool 以支持多个测试
    update_global_pool(pool);
}

/// 检查数据库结构是否需要更新
#[tauri::command]
pub async fn check_database_schema() -> Result<serde_json::Value, String> {
    let pool = get_database_pool();
    
    let chains_table_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chains'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;
    
    let tokens_table_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tokens'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;
    
    let rpc_table_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='rpc_providers'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;
    
    let contract_type_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('tokens') WHERE name = 'contract_type'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;
    
    let abi_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('tokens') WHERE name = 'abi'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;

    let ecosystem_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('chains') WHERE name = 'ecosystem'"
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| format!("检查数据库结构失败: {e}"))?;
    
    let schema_info = serde_json::json!({
        "db_exists": true,
        "chains_table_exists": chains_table_exists > 0,
        "tokens_table_exists": tokens_table_exists > 0,
        "rpc_table_exists": rpc_table_exists > 0,
        "contract_type_column_exists": contract_type_exists > 0,
        "abi_column_exists": abi_exists > 0,
        "ecosystem_column_exists": ecosystem_exists > 0,
        "needs_migration": false
    });
    
    Ok(schema_info)
}

/// 导出 public.db 数据库数据到 public_init.sql 文件
#[tauri::command]
pub async fn export_database_to_init_sql() -> Result<String, String> {
    let config = load_database_config();
    let enable_debug = config.enable_debug_log.unwrap_or(false);

    if enable_debug {
        println!("开始导出 public.db 数据到 public_init.sql...");
    }

    // 使用公开数据库连接池
    let pool = DualDatabaseManager::public_pool();

    let mut sql_content = String::new();

    // 生成带时间戳的文件头注释
    let now = chrono::Utc::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S UTC").to_string();

    sql_content.push_str("-- Wallet Manager 公开数据库初始化脚本\n");
    sql_content.push_str(&format!("-- 生成时间: {timestamp}\n"));
    sql_content.push_str("-- 此文件由系统自动生成，包含当前 public.db 的所有公开数据\n\n");

    // 只导出公开数据库的表（排除敏感数据表）
    let skip_tables = vec![
        "app_config",
        "wallet_groups",
        "wallets",
    ];

    let all_tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("获取表名失败: {e}"))?;

    // 过滤掉敏感数据表
    let tables: Vec<String> = all_tables
        .iter()
        .filter(|t| !skip_tables.contains(&t.as_str()))
        .cloned()
        .collect();

    if enable_debug {
        println!("将导出的表: {tables:?}");
    }

    // 导出表顺序：chains -> rpc_providers -> tokens -> 其他
    let mut ordered_tables = Vec::new();
    let priority_order = vec!["chains", "rpc_providers", "tokens"];

    for table_name in &priority_order {
        if tables.contains(&table_name.to_string()) {
            ordered_tables.push(table_name.to_string());
        }
    }

    for table in &tables {
        if !ordered_tables.contains(table) {
            ordered_tables.push(table.clone());
        }
    }

    for table in &ordered_tables {
        if enable_debug {
            println!("正在导出表: {table}");
        }

        // 获取表创建SQL
        let create_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name = ?"
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("获取表 {table} 结构失败: {e}"))?;

        sql_content.push_str(&format!("-- 创建{table}表\n"));
        sql_content.push_str(&create_sql);
        sql_content.push_str(";\n\n");

        // 获取表数据
        let rows = sqlx::query(&format!("SELECT * FROM {table}"))
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("获取表 {table} 数据失败: {e}"))?;

        if !rows.is_empty() {
            // 获取列名
            let columns: Vec<String> = sqlx::query_scalar(
                "SELECT name FROM pragma_table_info(?) ORDER BY cid"
            )
            .bind(table)
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("获取表 {table} 列名失败: {e}"))?;

            sql_content.push_str(&format!("-- 插入{table}表数据\n"));

            for row in rows {
                let mut values = Vec::new();
                for (i, _column) in columns.iter().enumerate() {
                    let value = if let Ok(v) = row.try_get::<Option<String>, _>(i) {
                        v
                    } else if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
                        v.map(|x| x.to_string())
                    } else if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
                        v.map(|x| x.to_string())
                    } else if let Ok(v) = row.try_get::<Option<bool>, _>(i) {
                        v.map(|x| if x { "1" } else { "0" }.to_string())
                    } else {
                        None
                    };

                    match value {
                        Some(v) => {
                            let escaped = v.replace("'", "''");
                            values.push(format!("'{escaped}'"));
                        }
                        None => values.push("NULL".to_string()),
                    }
                }

                let insert_sql = format!(
                    "INSERT OR IGNORE INTO {} ({}) VALUES ({});\n",
                    table,
                    columns.join(", "),
                    values.join(", ")
                );
                sql_content.push_str(&insert_sql);
            }
            sql_content.push('\n');
        }
    }

    // 导出索引
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='index' AND sql IS NOT NULL AND name NOT LIKE 'sqlite_%'"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| format!("获取索引失败: {e}"))?;

    if !indexes.is_empty() {
        sql_content.push_str("-- 创建索引\n");
        for index_sql in indexes {
            sql_content.push_str(&index_sql);
            sql_content.push_str(";\n");
        }
    }

    let init_sql_path = "data/public_init.sql";

    // 备份原文件（带日期时间戳）
    if std::path::Path::new(init_sql_path).exists() {
        let bak_path = format!(
            "data/public_init_{}.bak",
            now.format("%Y%m%d_%H%M%S")
        );
        std::fs::rename(init_sql_path, &bak_path)
            .map_err(|e| format!("备份原文件到 {bak_path} 失败: {e}"))?;
        if enable_debug {
            println!("已备份原文件到: {bak_path}");
        }
    }

    // 写入新文件
    std::fs::write(init_sql_path, &sql_content)
        .map_err(|e| format!("写入 public_init.sql 文件失败: {e}"))?;

    if enable_debug {
        println!("数据库导出完成，文件大小: {} 字节", sql_content.len());
    }

    Ok(format!(
        "public.db 数据已成功导出到 public_init.sql，共 {} 个表，文件大小: {} 字节",
        ordered_tables.len(),
        sql_content.len()
    ))
}
