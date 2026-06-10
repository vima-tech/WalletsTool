use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use futures::future::join_all as sol_join_all;
use crate::wallets_tool::security::SecureMemory;
use crate::database::chain_service::ChainService;
use crate::database::get_database_pool;
#[allow(deprecated)]
use solana_sdk::{
    signature::{Keypair, Signer},
    pubkey::Pubkey,
    system_instruction,
    transaction::Transaction,
    compute_budget::ComputeBudgetInstruction,
    program_pack::Pack,
};
use spl_token::state::Mint;
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account,
};
use std::str::FromStr;
use crate::wallets_tool::ecosystems::solana::provider::get_rpc_client;
use tauri::Emitter;
use serde_json::json;
use base64::Engine;

#[derive(Deserialize)]
pub struct TransferItem {
    pub private_key: SecureMemory,
    pub to_addr: String,
    pub amount: Option<String>,
}

#[derive(Deserialize)]
pub struct TransferConfig {
    pub transfer_amount: f64,
    pub gas_price: Option<u64>,
    pub contract_address: Option<String>,
    #[allow(dead_code)]
    pub amount_precision: Option<u8>,
    pub chain: Option<String>,
}

#[derive(Serialize)]
pub struct TransferResult {
    success: bool,
    tx_hash: Option<String>,
    error: Option<String>,
}

#[tauri::command]
pub async fn sol_transfer(
    _index: usize,
    item: TransferItem,
    config: TransferConfig,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<TransferResult, String> {
    let chain = config.chain.as_deref().unwrap_or("sol");
    let client = match get_rpc_client(chain, Some(&get_database_pool())).await {
        Ok(c) => c,
        Err(e) => return Ok(TransferResult { success: false, tx_hash: None, error: Some(format!("RPC连接失败: {e}")) }),
    };
    
    let keypair = item.private_key.use_secret(|secret_str| {
        let bytes = bs58::decode(secret_str).into_vec().map_err(|e| e.to_string())?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())??;

    let to_pubkey = Pubkey::from_str(&item.to_addr).map_err(|e| e.to_string())?;
    
    let input_amount = if config.transfer_amount < 0.0 {
        config.transfer_amount
    } else if let Some(amt) = &item.amount {
         if !amt.is_empty() { amt.parse::<f64>().unwrap_or(config.transfer_amount) } else { config.transfer_amount }
    } else {
        config.transfer_amount
    };

    let mut instructions = vec![];
    let lamports;

    if input_amount < 0.0 {
        // Send All Logic
        // 查询余额（带重试机制）
        let mut retry_count = 0;
        let max_retries = 3;
        let mut balance_result: Option<u64> = None;
        let mut last_error = String::new();
        
        while retry_count < max_retries && balance_result.is_none() {
            if retry_count > 0 {
                eprintln!("[DEBUG sol_transfer] Retrying balance query (attempt {}/{})", retry_count + 1, max_retries);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            match client.get_balance(&keypair.pubkey()).await {
                Ok(balance) => {
                    if balance > 0 {
                        balance_result = Some(balance);
                    } else {
                        last_error = "余额为0".to_string();
                    }
                },
                Err(e) => {
                    last_error = format!("RPC查询失败: {}", e);
                    eprintln!("[DEBUG sol_transfer] get_balance failed (attempt {}): {}", retry_count + 1, e);
                }
            }
            retry_count += 1;
        }
        
        let balance = balance_result.ok_or_else(|| {
            format!("查询余额失败（已重试{}次），最后错误: {}", max_retries, last_error)
        })?;
        
        let base_fee = 5000;
        let mut priority_fee = 0;
        // Simple transfer usually takes < 500 CU. Set 1000 to be safe and deterministic.
        let compute_unit_limit = 1000;

        if let Some(price) = config.gas_price {
            if price > 0 {
                instructions.push(ComputeBudgetInstruction::set_compute_unit_price(price));
                instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit));
                priority_fee = (compute_unit_limit as u64 * price + 999_999) / 1_000_000;
            }
        }

        let total_fee = base_fee + priority_fee;

        if balance <= total_fee {
            return Ok(TransferResult {
                success: false,
                tx_hash: None,
                error: Some(format!("余额不足以支付手续费 (余额: {} lamports, 手续费: {} lamports)", balance, total_fee)),
            });
        }

        lamports = balance - total_fee;
    } else {
        // Normal Transfer
        lamports = (input_amount * 1_000_000_000.0) as u64;
        
        if let Some(fee) = config.gas_price {
            if fee > 0 {
                 instructions.push(ComputeBudgetInstruction::set_compute_unit_price(fee));
            }
        }
    }

    instructions.push(system_instruction::transfer(&keypair.pubkey(), &to_pubkey, lamports));

    let recent_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction).await {
        Ok(sig) => Ok(TransferResult { success: true, tx_hash: Some(sig.to_string()), error: None }),
        Err(e) => Ok(TransferResult { success: false, tx_hash: None, error: Some(e.to_string()) }),
    }
}

#[tauri::command]
pub async fn sol_token_transfer(
    _index: usize,
    item: TransferItem,
    config: TransferConfig,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<TransferResult, String> {
    let chain = config.chain.as_deref().unwrap_or("sol");
    let client = match get_rpc_client(chain, Some(&get_database_pool())).await {
        Ok(c) => c,
        Err(e) => return Ok(TransferResult { success: false, tx_hash: None, error: Some(format!("RPC连接失败: {e}")) }),
    };
    let mint_str = config.contract_address.ok_or("Missing Mint Address")?;
    let mint = Pubkey::from_str(&mint_str).map_err(|_| "Invalid Mint Address")?;
    
    let keypair = item.private_key.use_secret(|secret_str| {
        let bytes = bs58::decode(secret_str).into_vec().map_err(|e| e.to_string())?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())??;

    let to_pubkey = Pubkey::from_str(&item.to_addr).map_err(|_| "Invalid To Address")?;
    
    let from_ata = get_associated_token_address(&keypair.pubkey(), &mint);
    let to_ata = get_associated_token_address(&to_pubkey, &mint);
    
    let amount_val = if config.transfer_amount < 0.0 {
        config.transfer_amount
    } else if let Some(amt) = &item.amount {
         if !amt.is_empty() { amt.parse::<f64>().unwrap_or(config.transfer_amount) } else { config.transfer_amount }
    } else {
        config.transfer_amount
    };
    
    // Debug log to help diagnose amount issues
    eprintln!("[DEBUG sol_token_transfer] item.amount: {:?}, config.transfer_amount: {}, amount_val: {}", 
              item.amount, config.transfer_amount, amount_val);
    
    let decimals = {
        // 自动获取代币精度 (忽略 config.amount_precision，因为它通常是前端UI的随机数保留位数，而非Token精度)
        let account_info = client.get_account(&mint).await.map_err(|e| format!("无法获取代币信息: {e}"))?;
        if account_info["value"].is_null() {
            return Ok(TransferResult { success: false, tx_hash: None, error: Some("代币Mint账户不存在".to_string()) });
        }
        let data_str = account_info["value"]["data"][0].as_str().ok_or("无效的代币数据格式")?;
        let data_bytes = base64::engine::general_purpose::STANDARD.decode(data_str)
            .map_err(|_| "Base64解码失败")?;
            
        let mint_state = Mint::unpack(&data_bytes).map_err(|_| "无法解析Mint数据")?;
        mint_state.decimals
    };

    let amount_u64 = if amount_val < 0.0 {
        // Send All Logic for Token
        // 查询ATA余额（带重试机制）
        let from_ata = get_associated_token_address(&keypair.pubkey(), &mint);
        eprintln!("[DEBUG sol_token_transfer] Send All mode - from_ata: {}, decimals: {}", from_ata, decimals);
        
        // 重试查询余额，最多3次
        let mut retry_count = 0;
        let max_retries = 3;
        let mut last_error = String::new();
        let mut balance_result: Option<u64> = None;
        
        while retry_count < max_retries && balance_result.is_none() {
            if retry_count > 0 {
                eprintln!("[DEBUG sol_token_transfer] Retrying balance query (attempt {}/{})", retry_count + 1, max_retries);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            
            match client.get_token_account_balance(&from_ata).await {
                Ok(balance_res) => {
                    eprintln!("[DEBUG sol_token_transfer] balance_res.amount: '{}', ui_amount: {:?}", 
                              balance_res.amount, balance_res.ui_amount);
                    // 优先使用 amount 字段（原始u64字符串），如果不存在则尝试从 uiAmount 计算（不推荐，有精度风险）
                    if !balance_res.amount.is_empty() {
                        if let Ok(amount) = balance_res.amount.parse::<u64>() {
                            if amount > 0 {
                                balance_result = Some(amount);
                            } else {
                                last_error = "余额为0".to_string();
                            }
                        } else {
                            last_error = "解析余额失败".to_string();
                        }
                    } else if let Some(ui_amt) = balance_res.ui_amount {
                        let calculated = (ui_amt * 10f64.powi(decimals as i32)) as u64;
                        if calculated > 0 {
                            balance_result = Some(calculated);
                        } else {
                            last_error = "ui_amount计算结果为0".to_string();
                        }
                    } else {
                        last_error = "余额数据为空".to_string();
                        eprintln!("[DEBUG sol_token_transfer] Both amount and ui_amount are empty!");
                    }
                },
                Err(e) => {
                    last_error = format!("RPC查询失败: {}", e);
                    eprintln!("[DEBUG sol_token_transfer] get_token_account_balance failed (attempt {}): {}", retry_count + 1, e);
                },
            }
            retry_count += 1;
        }
        
        match balance_result {
            Some(amount) => amount,
            None => {
                eprintln!("[DEBUG sol_token_transfer] All {} balance query attempts failed, last error: {}", max_retries, last_error);
                0
            }
        }
    } else {
        let calculated = (amount_val * 10f64.powi(decimals as i32)) as u64;
        eprintln!("[DEBUG sol_token_transfer] Normal mode - amount_val: {}, decimals: {}, calculated: {}", 
                  amount_val, decimals, calculated);
        calculated
    };
    
    // 检查金额是否为 0
    if amount_u64 == 0 {
        let error_msg = if amount_val < 0.0 {
            // Send All 模式下 amount_u64 为 0 的可能原因：
            // 1. 用户 ATA 账户不存在（没有代币余额）
            // 2. RPC 查询失败
            "转账金额为 0：请确认钱包已持有该代币且ATA账户已创建".to_string()
        } else if amount_val == 0.0 {
            "转账金额不能为 0：请检查转账金额设置".to_string()
        } else {
            format!("转账金额计算结果为 0：amount_val={}, decimals={}", amount_val, decimals)
        };
        eprintln!("[DEBUG sol_token_transfer] amount_u64 is 0, error_msg: {}", error_msg);
        return Ok(TransferResult { 
            success: false, 
            tx_hash: None, 
            error: Some(error_msg)
        });
    }

    let mut instructions = vec![];
    if let Some(fee) = config.gas_price {
        if fee > 0 {
             instructions.push(ComputeBudgetInstruction::set_compute_unit_price(fee));
        }
    }

    // Check if destination account exists
    let should_create_ata = match client.get_account(&to_ata).await {
        Ok(res) => res["value"].is_null(),
        Err(_) => true,
    };

    if should_create_ata {
        instructions.push(create_associated_token_account(
            &keypair.pubkey(),
            &to_pubkey,
            &mint,
            &spl_token::id(),
        ));
    }
    
    instructions.push(spl_token::instruction::transfer(
        &spl_token::id(),
        &from_ata,
        &to_ata,
        &keypair.pubkey(),
        &[],
        amount_u64,
    ).map_err(|e| e.to_string())?);

    let recent_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction).await {
        Ok(sig) => Ok(TransferResult { success: true, tx_hash: Some(sig.to_string()), error: None }),
        Err(e) => Ok(TransferResult { success: false, tx_hash: None, error: Some(e.to_string()) }),
    }
}

#[derive(Serialize)]
pub struct CheckResult {
    has_recent_transfer: bool,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn sol_check_recent_transfers(
    chain: String,
    private_key: SecureMemory,
    _target_address: String,
    start_timestamp: Option<i64>,
    _coin_type: String,
    _contract_address: Option<String>,
    _amount: Option<String>,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<CheckResult, String> {
    let client = match get_rpc_client(&chain, Some(&get_database_pool())).await {
        Ok(c) => c,
        Err(e) => return Err(format!("RPC连接失败: {e}")),
    };

    let keypair = private_key.use_secret(|secret_str| {
        let bytes = bs58::decode(secret_str).into_vec().map_err(|e| e.to_string())?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())??;

    // 获取最近的20条交易记录
    let signatures = client.get_signatures_for_address(&keypair.pubkey(), 20).await
        .map_err(|e| e.to_string())?;

    if let Some(start_ts) = start_timestamp {
        // 自动判断时间戳单位 (毫秒转秒)
        let start_seconds = if start_ts > 10_000_000_000 {
            start_ts / 1000
        } else {
            start_ts
        };

        for sig_info in signatures {
            if let Some(block_time) = sig_info.get("blockTime").and_then(|t| t.as_i64()) {
                // 如果交易时间晚于或等于开始时间，且没有错误（成功交易）
                if block_time >= start_seconds
                    && (sig_info.get("err").is_none() || sig_info["err"].is_null()) {
                         return Ok(CheckResult { has_recent_transfer: true });
                    }
            }
        }
    }

    Ok(CheckResult { has_recent_transfer: false })
}

#[tauri::command]
pub async fn sol_check_transactions_status_batch(
    chain: String,
    tx_hashes: Vec<String>,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<Vec<serde_json::Value>, String> {
    if tx_hashes.is_empty() {
        return Ok(vec![]);
    }

    let client = match get_rpc_client(&chain, Some(&get_database_pool())).await {
        Ok(c) => c,
        Err(e) => return Err(format!("RPC连接失败: {e}")),
    };

    let statuses = client.get_signature_statuses_batch(&tx_hashes).await
        .map_err(|e| e.to_string())?;
        
    let mut results = vec![];
    for (i, status_val) in statuses.iter().enumerate() {
        if i >= tx_hashes.len() { break; }
        let hash = &tx_hashes[i];
        
        let mut confirmed = false;
        let mut success = false;
        let mut error = serde_json::Value::Null;

        if !status_val.is_null() {
            if let Some(confirmation_status) = status_val.get("confirmationStatus").and_then(|s| s.as_str()) {
                if confirmation_status == "confirmed" || confirmation_status == "finalized" {
                    confirmed = true;
                    let err_val = status_val.get("err");
                    if err_val.is_none() || err_val.unwrap().is_null() {
                        success = true;
                    } else {
                        success = false;
                        error = err_val.unwrap().clone();
                    }
                }
            }
        }

        results.push(json!({
            "hash": hash,
            "status": {
                "confirmed": confirmed,
                "success": success,
                "error": error
            }
        }));
    }

    Ok(results)
}

#[tauri::command]
pub async fn sol_transfer_fast(
    index: usize,
    item: TransferItem,
    config: TransferConfig,
    chain_service: tauri::State<'_, ChainService>,
) -> Result<TransferResult, String> {
    sol_transfer(index, item, config, chain_service).await
}

#[tauri::command]
pub async fn sol_token_transfer_fast(
    index: usize,
    item: TransferItem,
    config: TransferConfig,
    chain_service: tauri::State<'_, ChainService>,
) -> Result<TransferResult, String> {
    sol_token_transfer(index, item, config, chain_service).await
}

#[derive(Deserialize, Debug, Clone)]
pub struct CoinConfig {
    pub coin_type: String, // "base" or "token"
    pub contract_address: Option<String>,
    #[allow(dead_code)]
    pub abi: Option<String>,
}

fn default_thread_count() -> usize { 5 }

#[derive(Deserialize)]
pub struct BalanceQueryParams {
    pub items: Vec<BalanceItem>,
    pub window_id: String,
    pub query_id: String,
    pub chain: Option<String>,
    pub coin_config: Option<CoinConfig>,
    #[serde(default = "default_thread_count")]
    pub thread_count: usize,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct BalanceItem {
    pub key: String,
    pub address: String,
    pub private_key: Option<String>, // We don't need private key for balance, but struct has it
    pub plat_balance: Option<String>,
    pub coin_balance: Option<String>,
    pub exec_status: String,
    pub error_msg: Option<String>,
}

#[tauri::command]
pub async fn sol_query_balances_with_updates<R: tauri::Runtime>(
    params: BalanceQueryParams,
    app_handle: tauri::AppHandle<R>,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<serde_json::Value, String> {
    let chain = params.chain.clone().unwrap_or_else(|| "sol".to_string());
    let client = match get_rpc_client(&chain, Some(&get_database_pool())).await {
        Ok(c) => c,
        Err(e) => return Err(format!("无法连接到 Solana RPC: {e}")),
    };

    let mint_pubkey: Option<Pubkey> = if let Some(config) = &params.coin_config {
        if config.coin_type == "token" {
            if let Some(addr) = &config.contract_address {
                if !addr.is_empty() {
                    match Pubkey::from_str(addr) {
                        Ok(pk) => Some(pk),
                        Err(_) => return Err("无效的代币合约地址".to_string()),
                    }
                } else { None }
            } else { None }
        } else { None }
    } else { None };

    let thread_count = params.thread_count.clamp(1, 50);
    let semaphore = Arc::new(Semaphore::new(thread_count));
    let client = Arc::new(client);
    let window_id = Arc::new(params.window_id.clone());
    let query_id = Arc::new(params.query_id.clone());
    let total = params.items.len();

    let results: Arc<tokio::sync::Mutex<Vec<Option<BalanceItem>>>> =
        Arc::new(tokio::sync::Mutex::new(vec![None; total]));

    let tasks: Vec<_> = params.items.into_iter().enumerate().map(|(idx, mut item)| {
        let sem = semaphore.clone();
        let client = client.clone();
        let ah = app_handle.clone();
        let wid = window_id.clone();
        let qid = query_id.clone();
        let results = results.clone();
        let mint = mint_pubkey;

        tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            // notify started
            item.exec_status = "1".to_string();
            let _ = ah.emit("balance_item_update", json!({
                "item": item.clone(), "window_id": *wid, "query_id": *qid
            }));

            if let Ok(pubkey) = Pubkey::from_str(&item.address) {
                match client.get_balance(&pubkey).await {
                    Ok(bal) => {
                        item.plat_balance = Some((bal as f64 / 1_000_000_000.0).to_string());
                        item.exec_status = "2".to_string();
                    }
                    Err(e) => {
                        item.exec_status = "3".to_string();
                        item.error_msg = Some(e.to_string());
                    }
                }

                if let Some(mint_pk) = mint {
                    if item.exec_status != "3" {
                        let ata = get_associated_token_address(&pubkey, &mint_pk);
                        match client.get_token_account_balance(&ata).await {
                            Ok(res) => {
                                item.coin_balance = Some(res.ui_amount_string.unwrap_or_else(|| "0".to_string()));
                            }
                            Err(_) => { item.coin_balance = Some("0".to_string()); }
                        }
                    }
                }
            } else {
                item.exec_status = "3".to_string();
                item.error_msg = Some("Invalid Address".to_string());
            }

            let _ = ah.emit("balance_item_update", json!({
                "item": item.clone(), "window_id": *wid, "query_id": *qid
            }));

            results.lock().await[idx] = Some(item);
            idx
        })
    }).collect();

    sol_join_all(tasks).await;

    let guard = results.lock().await;
    let items: Vec<BalanceItem> = guard.iter().filter_map(|v| v.clone()).collect();
    Ok(json!({ "success": true, "items": items }))
}

// ─── Batch SOL Transfer ────────────────────────────────────────────────────

#[derive(serde::Deserialize, Clone)]
pub struct BatchSolTransferItem {
    pub key: String,
    pub private_key: SecureMemory,
    pub to_addr: String,
    pub amount: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct BatchSolTransferConfig {
    pub transfer_amount: f64,
    pub gas_price: Option<u64>,
    pub contract_address: Option<String>,
    pub chain: Option<String>,
    pub delay: Option<[u64; 2]>,
    pub window_id: String,
    pub transfer_id: String,
}

#[derive(serde::Serialize)]
pub struct BatchSolTransferSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub stopped: bool,
}

#[tauri::command]
pub async fn batch_sol_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    items: Vec<BatchSolTransferItem>,
    config: BatchSolTransferConfig,
    thread_count: usize,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<BatchSolTransferSummary, String> {
    run_batch_sol_transfer(app_handle, items, config, thread_count, false).await
}

#[tauri::command]
pub async fn batch_sol_token_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    items: Vec<BatchSolTransferItem>,
    config: BatchSolTransferConfig,
    thread_count: usize,
    _chain_service: tauri::State<'_, ChainService>,
) -> Result<BatchSolTransferSummary, String> {
    run_batch_sol_transfer(app_handle, items, config, thread_count, true).await
}

async fn run_batch_sol_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    items: Vec<BatchSolTransferItem>,
    config: BatchSolTransferConfig,
    thread_count: usize,
    is_token: bool,
) -> Result<BatchSolTransferSummary, String> {
    // Solana does not use sequential nonces, so all items can run truly in parallel
    let window_id = config.window_id.clone();
    let total = items.len();
    let thread_count = thread_count.clamp(1, 100);
    let semaphore = Arc::new(Semaphore::new(thread_count));
    let config = Arc::new(config);

    use std::sync::atomic::{AtomicUsize, Ordering as AO};
    let success_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));

    let tasks: Vec<_> = items.into_iter().map(|front_item| {
        let sem = semaphore.clone();
        let cfg = config.clone();
        let ah = app_handle.clone();
        let wid = window_id.clone();
        let sc = success_count.clone();
        let fc = failed_count.clone();

        tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            if crate::wallets_tool::ecosystems::ethereum::transfer::get_stop_flag(&wid) {
                fc.fetch_add(1, AO::Relaxed);
                return;
            }

            let _ = ah.emit("transfer_item_update", json!({
                "key": front_item.key, "exec_status": "1", "tx_hash": null,
                "error_msg": "执行中...", "window_id": wid, "transfer_id": cfg.transfer_id,
            }));

            let actual_amount = if let Some(ref amt) = front_item.amount {
                if !amt.is_empty() { amt.parse::<f64>().unwrap_or(cfg.transfer_amount) } else { cfg.transfer_amount }
            } else { cfg.transfer_amount };

            let chain = cfg.chain.as_deref().unwrap_or("sol");
            let client = match get_rpc_client(chain, Some(&get_database_pool())).await {
                Ok(c) => c,
                Err(e) => {
                    let _ = ah.emit("transfer_item_update", json!({
                        "key": front_item.key, "exec_status": "3", "tx_hash": null,
                        "error_msg": format!("RPC连接失败: {e}"), "window_id": wid, "transfer_id": cfg.transfer_id,
                    }));
                    fc.fetch_add(1, AO::Relaxed);
                    return;
                }
            };

            let transfer_item = TransferItem {
                private_key: front_item.private_key.clone(),
                to_addr: front_item.to_addr.clone(),
                amount: front_item.amount.clone(),
            };
            let transfer_cfg = TransferConfig {
                transfer_amount: actual_amount,
                gas_price: cfg.gas_price,
                contract_address: if is_token { cfg.contract_address.clone() } else { None },
                amount_precision: None,
                chain: cfg.chain.clone(),
            };

            let result = if is_token {
                // Reuse existing token transfer logic by calling the core function
                sol_token_transfer_core(&client, transfer_item, transfer_cfg).await
            } else {
                sol_transfer_core(&client, transfer_item, transfer_cfg).await
            };

            match result {
                Ok(tx_hash) => {
                    let _ = ah.emit("transfer_item_update", json!({
                        "key": front_item.key, "exec_status": "2",
                        "tx_hash": tx_hash, "error_msg": tx_hash,
                        "window_id": wid, "transfer_id": cfg.transfer_id,
                    }));
                    sc.fetch_add(1, AO::Relaxed);
                }
                Err(e) => {
                    let _ = ah.emit("transfer_item_update", json!({
                        "key": front_item.key, "exec_status": "3", "tx_hash": null,
                        "error_msg": e, "window_id": wid, "transfer_id": cfg.transfer_id,
                    }));
                    fc.fetch_add(1, AO::Relaxed);
                }
            }
        })
    }).collect();

    sol_join_all(tasks).await;

    Ok(BatchSolTransferSummary {
        total,
        success: success_count.load(std::sync::atomic::Ordering::Relaxed),
        failed: failed_count.load(std::sync::atomic::Ordering::Relaxed),
        stopped: false,
    })
}

// Extract core logic from sol_transfer (without State dependency)
async fn sol_transfer_core(
    client: &crate::wallets_tool::ecosystems::solana::provider::SolanaProvider,
    item: TransferItem,
    config: TransferConfig,
) -> Result<String, String> {
    let keypair = item.private_key.use_secret(|s| {
        let bytes = bs58::decode(s).into_vec().map_err(|e| e.to_string())?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())??;

    let to_pubkey = Pubkey::from_str(&item.to_addr).map_err(|e| e.to_string())?;

    let input_amount = if config.transfer_amount < 0.0 {
        config.transfer_amount
    } else if let Some(ref amt) = item.amount {
        if !amt.is_empty() { amt.parse::<f64>().unwrap_or(config.transfer_amount) } else { config.transfer_amount }
    } else { config.transfer_amount };

    let mut instructions = vec![];
    let lamports;

    if input_amount < 0.0 {
        let balance = client.get_balance(&keypair.pubkey()).await.map_err(|e| e.to_string())?;
        let base_fee = 5000u64;
        let mut priority_fee = 0u64;
        let compute_unit_limit = 1000u32;
        if let Some(price) = config.gas_price {
            if price > 0 {
                instructions.push(ComputeBudgetInstruction::set_compute_unit_price(price));
                instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit));
                priority_fee = (compute_unit_limit as u64 * price + 999_999) / 1_000_000;
            }
        }
        let total_fee = base_fee + priority_fee;
        if balance <= total_fee {
            return Err(format!("余额不足以支付手续费 (余额: {balance} lamports)"));
        }
        lamports = balance - total_fee;
    } else {
        lamports = (input_amount * 1_000_000_000.0) as u64;
        if let Some(fee) = config.gas_price {
            if fee > 0 {
                instructions.push(ComputeBudgetInstruction::set_compute_unit_price(fee));
            }
        }
    }

    instructions.push(system_instruction::transfer(&keypair.pubkey(), &to_pubkey, lamports));
    let recent_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &instructions, Some(&keypair.pubkey()), &[&keypair], recent_blockhash,
    );
    client.send_and_confirm_transaction(&transaction).await
        .map(|sig| sig.to_string())
        .map_err(|e| e.to_string())
}

// Extract core logic from sol_token_transfer (without State dependency)
async fn sol_token_transfer_core(
    client: &crate::wallets_tool::ecosystems::solana::provider::SolanaProvider,
    item: TransferItem,
    config: TransferConfig,
) -> Result<String, String> {
    let mint_str = config.contract_address.ok_or("Missing Mint Address")?;
    let mint = Pubkey::from_str(&mint_str).map_err(|_| "Invalid Mint Address".to_string())?;

    let keypair = item.private_key.use_secret(|s| {
        let bytes = bs58::decode(s).into_vec().map_err(|e| e.to_string())?;
        Keypair::try_from(bytes.as_slice()).map_err(|e| e.to_string())
    }).map_err(|e| e.to_string())??;

    let to_pubkey = Pubkey::from_str(&item.to_addr).map_err(|_| "Invalid To Address".to_string())?;
    let from_ata = get_associated_token_address(&keypair.pubkey(), &mint);
    let to_ata = get_associated_token_address(&to_pubkey, &mint);

    let amount_val = if config.transfer_amount < 0.0 {
        config.transfer_amount
    } else if let Some(ref amt) = item.amount {
        if !amt.is_empty() { amt.parse::<f64>().unwrap_or(config.transfer_amount) } else { config.transfer_amount }
    } else { config.transfer_amount };

    let account_info = client.get_account(&mint).await.map_err(|e| format!("无法获取代币信息: {e}"))?;
    let data_str = account_info["value"]["data"][0].as_str().ok_or("无效的代币数据格式")?;
    let data_bytes = base64::engine::general_purpose::STANDARD.decode(data_str).map_err(|_| "Base64解码失败")?;
    let mint_state = Mint::unpack(&data_bytes).map_err(|_| "无法解析Mint数据")?;
    let decimals = mint_state.decimals;

    let amount_u64 = if amount_val < 0.0 {
        let res = client.get_token_account_balance(&from_ata).await.map_err(|e| e.to_string())?;
        res.amount.parse::<u64>().map_err(|_| "解析余额失败".to_string())?
    } else {
        (amount_val * 10f64.powi(decimals as i32)) as u64
    };

    if amount_u64 == 0 { return Err("转账金额不能为0".to_string()); }

    let mut instructions = vec![];
    if let Some(fee) = config.gas_price {
        if fee > 0 { instructions.push(ComputeBudgetInstruction::set_compute_unit_price(fee)); }
    }

    let should_create_ata = match client.get_account(&to_ata).await {
        Ok(res) => res["value"].is_null(),
        Err(_) => true,
    };
    if should_create_ata {
        instructions.push(create_associated_token_account(
            &keypair.pubkey(), &to_pubkey, &mint, &spl_token::id(),
        ));
    }

    instructions.push(spl_token::instruction::transfer(
        &spl_token::id(), &from_ata, &to_ata, &keypair.pubkey(), &[], amount_u64,
    ).map_err(|e| e.to_string())?);

    let recent_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &instructions, Some(&keypair.pubkey()), &[&keypair], recent_blockhash,
    );
    client.send_and_confirm_transaction(&transaction).await
        .map(|sig| sig.to_string())
        .map_err(|e| e.to_string())
}
