use serde::{Deserialize, Serialize};
use alloy_provider::Provider;
use alloy_primitives::{Address, U256, TxKind};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_signer_local::PrivateKeySigner;
use alloy_signer::Signer;
use std::sync::Arc;
use rand::Rng;
use tokio::sync::Semaphore;
use futures::future::join_all;
use tokio::time::{sleep, Duration};
use tauri::Emitter;
use super::transfer::{TransferConfig, TransferItem, TransferResult, TransferUtils, create_provider, create_signer_provider, get_rpc_config, FastTransferResult, get_stop_flag};
use crate::wallets_tool::ecosystems::ethereum::provider::{ProviderUtils, AlloyProvider};
use hex;
use super::alloy_utils::{format_wei_to_ether, format_wei_to_gwei, parse_decimal_to_units, u256_to_f64};




#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenTransferConfig {
    pub chain: String,
    pub contract_address: String,
    pub delay: [u64; 2],
    pub transfer_type: String,
    pub transfer_amount: f64,
    pub transfer_amount_list: [f64; 2],
    pub left_amount_list: [f64; 2],
    pub amount_precision: u8,
    pub limit_type: String,
    pub limit_count: u64,
    pub limit_count_list: [u64; 2],
    pub gas_price_type: String,
    pub gas_price: f64,
    pub gas_price_rate: f64,
    pub max_gas_price: f64,
    pub error_retry: String,
    pub error_count_limit: u32,
    #[serde(default)]
    pub window_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,
    pub decimals: u8,
    pub balance: String,
}

pub struct TokenTransferUtils;

fn address_to_abi_param(address: Address) -> String {
    format!("{:0>64}", hex::encode(address))
}

impl TokenTransferUtils {
    pub async fn get_contract_gas_limit(
        config: &TokenTransferConfig,
        provider: Arc<AlloyProvider>,
        contract_address: Address,
        wallet_address: Address,
        to_address: Address,
        transfer_amount: U256,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        let transfer_config = TransferConfig {
            chain: config.chain.clone(),
            delay: config.delay,
            transfer_type: config.transfer_type.clone(),
            transfer_amount: config.transfer_amount,
            transfer_amount_list: config.transfer_amount_list,
            left_amount_list: config.left_amount_list,
            amount_precision: config.amount_precision,
            limit_type: config.limit_type.clone(),
            limit_count: config.limit_count,
            limit_count_list: config.limit_count_list,
            gas_price_type: config.gas_price_type.clone(),
            gas_price: config.gas_price,
            gas_price_rate: config.gas_price_rate,
            max_gas_price: config.max_gas_price,
            error_retry: config.error_retry.clone(),
            error_count_limit: config.error_count_limit,
            window_id: config.window_id.clone(),
        };

        let method_id = "a9059cbb";
        let to_param = address_to_abi_param(to_address);
        let amount_param = format!("{:0>64}", format!("{:x}", transfer_amount));
        let data = format!("0x{method_id}{to_param}{amount_param}");
        let input = data
            .parse::<alloy_primitives::Bytes>()
            .map_err(|e| format!("解析合约调用数据失败: {e}"))?;

        let estimate_tx = TransactionRequest {
            from: Some(wallet_address),
            to: Some(TxKind::Call(contract_address)),
            input: input.into(),
            value: Some(U256::from(0)),
            ..Default::default()
        };

        TransferUtils::get_gas_limit_for_tx(&transfer_config, provider, estimate_tx, false).await
            .map_err(|e| format!("获取代币合约Gas Limit失败: {e}").into())
    }

    pub async fn get_token_balance(
        provider: &AlloyProvider,
        contract_address: Address,
        wallet_address: Address,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        let method_id = "70a08231";
        let address_param = address_to_abi_param(wallet_address);
        let data = format!("0x{method_id}{address_param}");
        
        let tx = TransactionRequest {
            to: Some(TxKind::Call(contract_address)),
            input: data.parse::<alloy_primitives::Bytes>().unwrap().into(),
            ..Default::default()
        };
        
        let result = provider.call(tx).await?;
        
        if result.is_empty() {
            return Ok(U256::from(0));
        }
        
        let result_hex = hex::encode(result);
        let balance_hex = result_hex.trim_start_matches("0x");
        let balance = U256::from_str_radix(balance_hex, 16)
            .map_err(|e| format!("解析代币余额失败: {e}"))?;
        
        Ok(balance)
    }

    pub async fn get_token_info(
        provider: &AlloyProvider,
        contract_address: Address,
    ) -> Result<(String, u8), Box<dyn std::error::Error>> {
        let decimals_method = "313ce567";
        let decimals_data = format!("0x{decimals_method}");
        
        let tx = TransactionRequest {
            to: Some(TxKind::Call(contract_address)),
            input: decimals_data.parse::<alloy_primitives::Bytes>().unwrap().into(),
            ..Default::default()
        };
        
        let decimals_result = provider.call(tx).await?;
        if decimals_result.is_empty() {
            return Err("代币精度返回为空".into());
        }
        let decimals_hex_full = hex::encode(&decimals_result);
        let decimals_hex = if decimals_hex_full.len() > 64 {
            &decimals_hex_full[decimals_hex_full.len() - 64..]
        } else {
            decimals_hex_full.as_str()
        };
        let decimals_u256 = U256::from_str_radix(decimals_hex, 16)
            .map_err(|e| format!("解析代币精度失败: {e}"))?;
        let decimals_u64 = decimals_u256.to::<u64>();
        if decimals_u64 > u8::MAX as u64 {
            return Err(format!("代币精度超出范围: {decimals_u64}").into());
        }
        let decimals = decimals_u64 as u8;
        
        let symbol_method = "95d89b41";
        let symbol_data = format!("0x{symbol_method}");
        
        let tx = TransactionRequest {
            to: Some(TxKind::Call(contract_address)),
            input: symbol_data.parse::<alloy_primitives::Bytes>().unwrap().into(),
            ..Default::default()
        };
        
        let symbol_result = provider.call(tx).await?;
        if symbol_result.is_empty() {
            return Err("代币符号返回为空".into());
        }
        let raw = symbol_result.as_ref();
        let symbol = if raw.len() == 32 {
            let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
            String::from_utf8(raw[..end].to_vec()).map_err(|e| format!("代币符号转换失败: {e}"))?
        } else if raw.len() >= 64 {
            let len_hex = hex::encode(&raw[32..64]);
            let len_u256 = U256::from_str_radix(&len_hex, 16)
                .map_err(|e| format!("解析代币符号长度失败: {e}"))?;
            let len = len_u256.to::<usize>();
            if raw.len() < 64 + len {
                return Err("代币符号返回值长度不足".into());
            }
            String::from_utf8(raw[64..64 + len].to_vec()).map_err(|e| format!("代币符号转换失败: {e}"))?
        } else {
            return Err("代币符号返回值格式不支持".into());
        };
        
        Ok((symbol, decimals))
    }

    pub async fn token_transfer<R: tauri::Runtime>(
        app_handle: tauri::AppHandle<R>,
        index: usize,
        item: TransferItem,
        config: TokenTransferConfig,
    ) -> Result<TransferResult, String> {
        match token_transfer_internal(app_handle, index, item, config).await {
            Ok(tx_hash) => Ok(TransferResult {
                success: true,
                tx_hash: Some(tx_hash),
                error: None,
            }),
            Err(e) => Ok(TransferResult {
                success: false,
                tx_hash: None,
                error: Some(e.to_string()),
            }),
        }
    }
}

async fn token_transfer_internal<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    mut item: TransferItem,
    config: TokenTransferConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    item.retry_flag = false;
    let window_id = config.window_id.as_deref().unwrap_or("");
    
    let signer = item.private_key.use_secret(|pk| {
        if pk.trim().is_empty() {
             return Err("私钥不能为空！".to_string());
        }
        let private_key = if pk.starts_with("0x") || pk.starts_with("0X") {
            &pk[2..]
        } else {
            pk
        };
        private_key.parse::<PrivateKeySigner>().map_err(|e| e.to_string())
    })
    .map_err(|e| format!("私钥解密失败: {e}"))?
    .map_err(|e| format!("私钥格式错误: {e}"))?;
    
    let chain_id = match ProviderUtils::get_chain_id(&config.chain).await {
        Ok(id) => id,
        Err(_) => {
            match get_rpc_config(&config.chain).await {
                Some(c) => c.chain_id,
                None => {
                    return Err(format!("无法获取链 '{}' 的配置信息", config.chain).into());
                }
            }
        }
    };
    let signer = signer.with_chain_id(Some(chain_id));
    let wallet_address = signer.address();
    
    if item.to_addr.trim().is_empty() {
        return Err("目标地址不能为空".into());
    }
    let to_address: Address = item.to_addr.parse()
        .map_err(|e| format!("目标地址格式错误: {e}"))?;
    
    let contract_address: Address = config.contract_address.parse()
        .map_err(|e| format!("合约地址格式错误: {e}"))?;
    
    let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
        match rpc_config.get_random_rpc() {
            Ok(url) => url.to_string(),
            Err(e) => return Err(format!("获取RPC地址失败: {e}").into()),
        }
    } else {
        return Err(format!("无法获取链 '{}' 的RPC配置", config.chain).into());
    };
    
    let provider = create_provider(&config.chain, config.window_id.as_deref()).await
        .map_err(|e| format!("获取RPC提供商失败: {e}"))?;
    
    let balance = TokenTransferUtils::get_token_balance(&provider, contract_address, wallet_address).await
        .map_err(|e| format!("获取代币余额失败 (RPC: {rpc_url}): {e}"))?;
    
    let decimals = if let Ok((_, d)) = TokenTransferUtils::get_token_info(&provider, contract_address).await {
        d
    } else {
        18
    };
    
    let balance_wei = u256_to_f64(balance);
    let balance_tokens = balance_wei / 10f64.powi(decimals as i32);
    println!("序号：{index}, 当前代币余额为: {balance_tokens} ({balance} wei)");
    
    let gas_price = TransferUtils::get_gas_price(
        &TransferConfig {
            chain: config.chain.clone(),
            delay: config.delay,
            transfer_type: config.transfer_type.clone(),
            transfer_amount: config.transfer_amount,
            transfer_amount_list: config.transfer_amount_list,
            left_amount_list: config.left_amount_list,
            amount_precision: config.amount_precision,
            limit_type: config.limit_type.clone(),
            limit_count: config.limit_count,
            limit_count_list: config.limit_count_list,
            gas_price_type: config.gas_price_type.clone(),
            gas_price: config.gas_price,
            gas_price_rate: config.gas_price_rate,
            max_gas_price: config.max_gas_price,
            error_retry: config.error_retry.clone(),
            error_count_limit: config.error_count_limit,
            window_id: config.window_id.clone(),
        },
        provider.clone()
    ).await
        .map_err(|e| format!("获取Gas Price失败 (RPC: {rpc_url}): {e}"))?;
    
    if gas_price.is_zero() {
        return Err("获取到的 gas price 为0".into());
    }
    
    let transfer_amount = match config.transfer_type.as_str() {
        "1" => balance,
        "2" => {
            let formatted_amount = format!(
                "{:.precision$}",
                config.transfer_amount,
                precision = config.amount_precision as usize
            );
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "3" => {
            let mut rng = rand::thread_rng();
            let random_amount = rng.gen_range(config.transfer_amount_list[0]..=config.transfer_amount_list[1]);
            let formatted_amount = format!("{:.precision$}", random_amount, precision = config.amount_precision as usize);
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "4" => {
            let balance_ether = balance_wei / 10f64.powi(decimals as i32);
            let available_balance = balance_ether;
            
            if available_balance <= config.left_amount_list[1] {
                return Err(format!("当前可用余额为：{}，无法满足最大剩余数量 {} 要求", available_balance, config.left_amount_list[1]).into());
            }
            
            let mut rng = rand::thread_rng();
            let left_amount = rng.gen_range(config.left_amount_list[0]..=config.left_amount_list[1]);
            let transfer_amount_f64 = available_balance - left_amount;
            
            if transfer_amount_f64 <= 0.0 {
                return Err("计算转账金额为负数或零，不做转账操作！".into());
            }
            
            let formatted_amount = format!("{:.precision$}", transfer_amount_f64, precision = config.amount_precision as usize);
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount > balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        _ => return Err("无效的转账类型".into()),
    };

    if transfer_amount.is_zero() {
        return Err("转账数量为0，不做转账操作！".into());
    }
    
    let gas_limit = TokenTransferUtils::get_contract_gas_limit(&config, provider.clone(), contract_address, wallet_address, to_address, transfer_amount).await?;
    let actual_gas_fee = gas_price * gas_limit;
    
    println!("序号：{}, 转账数量为: {}, gas_limit: {}, gas_price: {} gwei, gas_fee: {} ETH", 
        index, transfer_amount, gas_limit, format_wei_to_gwei(gas_price), format_wei_to_ether(actual_gas_fee));
    
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": "发送交易中...",
        "exec_status": "1"
    }));
    
    let method_id = "a9059cbb";
    let to_param = address_to_abi_param(to_address);
    let amount_param = format!("{:0>64}", format!("{:x}", transfer_amount));
    let data = format!("0x{method_id}{to_param}{amount_param}");
    
    let tx = TransactionRequest {
        to: Some(TxKind::Call(contract_address)),
        input: data.parse::<alloy_primitives::Bytes>().unwrap().into(),
        value: Some(U256::from(0)),
        gas: Some(gas_limit.to::<u64>()),
        gas_price: Some(gas_price.to::<u128>()),
        ..Default::default()
    };
    
    // 再次检查停止状态 - 在发送交易之前 (最关键的拦截点)
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }
    
    let signer_provider = create_signer_provider(&config.chain, config.window_id.as_deref(), &signer).await?;
    let pending_tx = signer_provider.send_transaction(tx).await
        .map_err(|e| format!("发送交易失败: {e}"))?;
    
    let tx_hash = *pending_tx.tx_hash();
    let tx_hash_str = format!("{tx_hash:?}");
    
    println!("序号：{index}, 交易 hash 为：{tx_hash:?}");
    
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": format!("已提交，等待确认: {}", &tx_hash_str[..20]),
        "exec_status": "1"
    }));
    
    Ok(tx_hash_str)
}

#[tauri::command]
pub async fn query_token_balance(
    chain: String,
    contract_address: String,
    address: String,
) -> Result<String, String> {
    let provider = create_provider(&chain, None).await
        .map_err(|e| format!("获取RPC提供商失败: {e}"))?;
    
    let contract_addr: Address = contract_address.parse()
        .map_err(|e| format!("合约地址格式错误: {e}"))?;
    let wallet_addr: Address = address.parse()
        .map_err(|e| format!("钱包地址格式错误: {e}"))?;
    
    let balance = TokenTransferUtils::get_token_balance(&provider, contract_addr, wallet_addr).await
        .map_err(|e| e.to_string())?;
    
    let decimals = if let Ok((_, d)) = TokenTransferUtils::get_token_info(&provider, contract_addr).await {
        d
    } else {
        18
    };
    
    let balance_wei = u256_to_f64(balance);
    let balance_tokens = balance_wei / 10f64.powi(decimals as i32);
    Ok(format!("{balance_tokens:.6}"))
}

#[tauri::command]
pub async fn get_token_info(
    chain: String,
    contract_address: String,
) -> Result<TokenInfo, String> {
    let provider = create_provider(&chain, None).await
        .map_err(|e| format!("获取RPC提供商失败: {e}"))?;
    
    let contract_addr: Address = contract_address.parse()
        .map_err(|e| format!("合约地址格式错误: {e}"))?;
    
    let (symbol, decimals) = TokenTransferUtils::get_token_info(&provider, contract_addr).await
        .map_err(|e| e.to_string())?;
    
    let wallet_address = "0x0000000000000000000000000000000000000000".parse::<Address>()
        .map_err(|e| format!("地址解析错误: {e}"))?;
    
    let balance = TokenTransferUtils::get_token_balance(&provider, contract_addr, wallet_address).await
        .map_err(|e| e.to_string())?;
    
    let balance_wei = u256_to_f64(balance);
    let balance_tokens = balance_wei / 10f64.powi(decimals as i32);
    
    Ok(TokenInfo {
        symbol,
        decimals,
        balance: format!("{balance_tokens:.6}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_to_abi_param_keeps_full_address() {
        let address: Address = "0x042614ef766f741dcd521035c20246ba9220e73d"
            .parse()
            .unwrap();
        let param = address_to_abi_param(address);
        assert_eq!(param.len(), 64);
        assert!(param[..24].chars().all(|c| c == '0'));
        assert_eq!(
            &param[24..],
            "042614ef766f741dcd521035c20246ba9220e73d"
        );
    }
}

#[tauri::command]
pub async fn token_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    item: TransferItem,
    config: TokenTransferConfig,
) -> Result<TransferResult, String> {
    TokenTransferUtils::token_transfer(app_handle, index, item, config).await
}

#[tauri::command]
pub async fn token_transfer_fast<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    item: TransferItem,
    config: TokenTransferConfig,
) -> Result<FastTransferResult, String> {
    match token_transfer_fast_internal(app_handle, index, item, config).await {
        Ok(tx_hash) => Ok(FastTransferResult {
            success: true,
            tx_hash: Some(tx_hash),
            error: None,
        }),
        Err(e) => Ok(FastTransferResult {
            success: false,
            tx_hash: None,
            error: Some(e.to_string()),
        }),
    }
}

async fn token_transfer_fast_internal<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    mut item: TransferItem,
    config: TokenTransferConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    item.retry_flag = false;
    let window_id = config.window_id.as_deref().unwrap_or("");
    
    // 0. 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    let signer = item.private_key.use_secret(|pk| {
        if pk.trim().is_empty() {
             return Err("私钥不能为空！".to_string());
        }
        let private_key = if pk.starts_with("0x") || pk.starts_with("0X") {
            &pk[2..]
        } else {
            pk
        };
        private_key.parse::<PrivateKeySigner>().map_err(|e| e.to_string())
    })
    .map_err(|e| format!("私钥解密失败: {e}"))?
    .map_err(|e| format!("私钥格式错误: {e}"))?;
    
    // 复用 Provider 逻辑
    let provider = create_provider(&config.chain, config.window_id.as_deref()).await
        .map_err(|e| format!("获取RPC提供商失败: {e}"))?;

    let chain_id = match ProviderUtils::get_chain_id(&config.chain).await {
        Ok(id) => id,
        Err(_) => {
            match provider.get_chain_id().await {
                Ok(id) => id,
                Err(_) => {
                    match get_rpc_config(&config.chain).await {
                        Some(c) => c.chain_id,
                        None => {
                            return Err(format!("无法获取链 '{}' 的配置信息", config.chain).into());
                        }
                    }
                }
            }
        }
    };
    let signer = signer.with_chain_id(Some(chain_id));
    let wallet_address = signer.address();
    
    if item.to_addr.trim().is_empty() {
        return Err("目标地址不能为空".into());
    }
    let to_address: Address = item.to_addr.parse()
        .map_err(|e| format!("目标地址格式错误: {e}"))?;
    
    let contract_address: Address = config.contract_address.parse()
        .map_err(|e| format!("代币合约地址格式错误: {e}"))?;
    
    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    let (_symbol, decimals) = TokenTransferUtils::get_token_info(&provider, contract_address).await
        .map_err(|e| format!("获取代币信息失败: {e}"))?;
    
    let balance = TokenTransferUtils::get_token_balance(&provider, contract_address, wallet_address).await
        .map_err(|e| format!("获取钱包代币余额失败: {e}"))?;
    
    let balance_wei = u256_to_f64(balance);

    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    let gas_price = TransferUtils::get_gas_price(&TransferConfig {
        chain: config.chain.clone(),
        delay: config.delay,
        transfer_type: config.transfer_type.clone(),
        transfer_amount: config.transfer_amount,
        transfer_amount_list: config.transfer_amount_list,
        left_amount_list: config.left_amount_list,
        amount_precision: config.amount_precision,
        limit_type: config.limit_type.clone(),
        limit_count: config.limit_count,
        limit_count_list: config.limit_count_list,
        gas_price_type: config.gas_price_type.clone(),
        gas_price: config.gas_price,
        gas_price_rate: config.gas_price_rate,
        max_gas_price: config.max_gas_price,
        error_retry: config.error_retry.clone(),
        error_count_limit: config.error_count_limit,
        window_id: config.window_id.clone(),
    }, provider.clone()).await
        .map_err(|e| format!("获取Gas Price失败: {e}"))?;
        
    if gas_price.is_zero() {
        return Err("获取到的 gas price 为0".into());
    }
    
    // 计算转账金额 (修复：支持多种转账类型)
    let transfer_amount = match config.transfer_type.as_str() {
        "1" => balance,
        "2" => {
            let formatted_amount = format!(
                "{:.precision$}",
                config.transfer_amount,
                precision = config.amount_precision as usize
            );
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "3" => {
            let mut rng = rand::thread_rng();
            let random_amount = rng.gen_range(config.transfer_amount_list[0]..=config.transfer_amount_list[1]);
            let formatted_amount = format!("{:.precision$}", random_amount, precision = config.amount_precision as usize);
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "4" => {
            let balance_ether = balance_wei / 10f64.powi(decimals as i32);
            let available_balance = balance_ether;
            
            if available_balance <= config.left_amount_list[1] {
                return Err(format!("当前可用余额为：{}，无法满足最大剩余数量 {} 要求", available_balance, config.left_amount_list[1]).into());
            }
            
            let mut rng = rand::thread_rng();
            let left_amount = rng.gen_range(config.left_amount_list[0]..=config.left_amount_list[1]);
            let transfer_amount_f64 = available_balance - left_amount;
            
            if transfer_amount_f64 <= 0.0 {
                return Err("计算转账金额为负数或零，不做转账操作！".into());
            }
            
            let formatted_amount = format!("{:.precision$}", transfer_amount_f64, precision = config.amount_precision as usize);
            let amount = parse_decimal_to_units(&formatted_amount, decimals)
                .map_err(|e| format!("解析转账数量失败: {e}"))?;
            if amount.is_zero() {
                return Err("转账数量过小，计算后为0".into());
            }
            if amount > balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        _ => return Err("无效的转账类型".into()),
    };

    if transfer_amount.is_zero() {
        return Err("转账数量为0，不做转账操作！".into());
    }
    
    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }
    
    let gas_limit = TokenTransferUtils::get_contract_gas_limit(&config, provider.clone(), contract_address, wallet_address, to_address, transfer_amount).await?;
    
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": "发送交易中...",
        "exec_status": "1"
    }));
    
    let method_id = "a9059cbb";
    let to_param = address_to_abi_param(to_address);
    let amount_param = format!("{:0>64}", format!("{:x}", transfer_amount));
    let data = format!("0x{method_id}{to_param}{amount_param}");
    
    let tx = TransactionRequest {
        to: Some(TxKind::Call(contract_address)),
        input: data.parse::<alloy_primitives::Bytes>().unwrap().into(),
        value: Some(U256::from(0)),
        gas: Some(gas_limit.to::<u64>()),
        gas_price: Some(gas_price.to::<u128>()),
        ..Default::default()
    };
    
    // 再次检查停止状态 - 在发送交易之前 (最关键的拦截点)
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    let signer_provider = create_signer_provider(&config.chain, config.window_id.as_deref(), &signer).await?;
    let pending_tx = signer_provider.send_transaction(tx).await
        .map_err(|e| format!("发送交易失败: {e}"))?;
    
    let tx_hash = *pending_tx.tx_hash();
    let tx_hash_str = format!("{tx_hash:?}");
    
    println!("[狂暴模式] 序号：{index}, 交易已提交，hash: {tx_hash_str}");

    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": format!("已提交，等待确认: {}", &tx_hash_str[..20]),
        "exec_status": "1"
    }));

    Ok(tx_hash_str)
}

// ─── Batch Token Transfer ──────────────────────────────────────────────────

use super::transfer::{BatchTransferItemFront, BatchTransferSummary, check_transaction_status_internal, reset_stop_flag};

#[derive(Debug, serde::Deserialize)]
pub struct BatchTokenTransferParams {
    pub items: Vec<BatchTransferItemFront>,
    pub config: TokenTransferConfig,
    pub thread_count: usize,
    pub fury_mode: bool,
    pub transfer_id: String,
}

#[tauri::command]
pub async fn batch_token_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    params: BatchTokenTransferParams,
) -> Result<BatchTransferSummary, String> {
    let window_id = params.config.window_id.clone().unwrap_or_default();
    reset_stop_flag(&window_id);

    let fury_mode = params.fury_mode || params.thread_count > 90;
    let thread_count = params.thread_count.clamp(1, 200);
    let total_items = params.items.len();

    let mut wallet_groups: Vec<Vec<BatchTransferItemFront>> = Vec::new();
    {
        let mut addr_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for item in params.items {
            let addr = item.address.to_lowercase();
            let idx = *addr_map.entry(addr).or_insert_with(|| {
                wallet_groups.push(Vec::new());
                wallet_groups.len() - 1
            });
            wallet_groups[idx].push(item);
        }
    }

    let semaphore = Arc::new(Semaphore::new(thread_count));
    let config = Arc::new(params.config);
    let transfer_id = Arc::new(params.transfer_id);

    use std::sync::atomic::{AtomicUsize, Ordering as AO};
    let success_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));

    let tasks: Vec<_> = wallet_groups.into_iter().map(|group| {
        let sem = semaphore.clone();
        let cfg = config.clone();
        let tid = transfer_id.clone();
        let ah = app_handle.clone();
        let wid = window_id.clone();
        let sc = success_count.clone();
        let fc = failed_count.clone();

        tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let group_len = group.len();

            for (i, front_item) in group.into_iter().enumerate() {
                if get_stop_flag(&wid) { break; }

                let mut item_cfg = (*cfg).clone();
                if let Some(ref amt) = front_item.amount {
                    if !amt.is_empty() {
                        if let Ok(v) = amt.parse::<f64>() { item_cfg.transfer_amount = v; }
                    }
                }

                let transfer_item = TransferItem {
                    private_key: front_item.private_key.clone(),
                    to_addr: front_item.to_addr.clone(),
                    error_msg: String::new(),
                    retry_flag: false,
                };

                let _ = ah.emit("transfer_item_update", serde_json::json!({
                    "key": front_item.key, "exec_status": "1", "tx_hash": null,
                    "error_msg": if fury_mode { "正在提交..." } else { "执行中..." },
                    "window_id": wid, "transfer_id": *tid,
                }));

                if fury_mode {
                    let fast_result: Result<String, String> = token_transfer_fast_internal(ah.clone(), 0, transfer_item, item_cfg.clone()).await.map_err(|e| e.to_string());
                    match fast_result {
                        Ok(tx_hash) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key, "exec_status": "4",
                                "tx_hash": tx_hash, "error_msg": "已提交，等待上链确认...",
                                "window_id": wid, "transfer_id": *tid,
                            }));
                            let chain = item_cfg.chain.clone();
                            let start = std::time::Instant::now();
                            let tx_copy = tx_hash.clone();
                            loop {
                                if start.elapsed() > std::time::Duration::from_secs(90) {
                                    let _ = ah.emit("transfer_item_update", serde_json::json!({
                                        "key": front_item.key, "exec_status": "3",
                                        "tx_hash": tx_copy, "error_msg": "上链确认超时",
                                        "window_id": wid, "transfer_id": *tid,
                                    }));
                                    fc.fetch_add(1, AO::Relaxed); break;
                                }
                                if get_stop_flag(&wid) { fc.fetch_add(1, AO::Relaxed); break; }
                                match check_transaction_status_internal(chain.clone(), tx_copy.clone()).await.map_err(|e| e.to_string()) {
                                    Ok(s) if s.confirmed => {
                                        let ok = s.success == Some(true);
                                        let _ = ah.emit("transfer_item_update", serde_json::json!({
                                            "key": front_item.key,
                                            "exec_status": if ok { "2" } else { "3" },
                                            "tx_hash": tx_copy,
                                            "error_msg": if ok { tx_copy.clone() } else { "链上执行失败".to_string() },
                                            "window_id": wid, "transfer_id": *tid,
                                        }));
                                        if ok { sc.fetch_add(1, AO::Relaxed); } else { fc.fetch_add(1, AO::Relaxed); }
                                        break;
                                    }
                                    _ => { sleep(Duration::from_secs(2)).await; }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key, "exec_status": "3", "tx_hash": null,
                                "error_msg": e.to_string(), "window_id": wid, "transfer_id": *tid,
                            }));
                            fc.fetch_add(1, AO::Relaxed);
                        }
                    }
                } else {
                    match token_transfer_internal(ah.clone(), 0, transfer_item, item_cfg.clone()).await {
                        Ok(tx_hash) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key, "exec_status": "2",
                                "tx_hash": tx_hash, "error_msg": tx_hash,
                                "window_id": wid, "transfer_id": *tid,
                            }));
                            sc.fetch_add(1, AO::Relaxed);
                        }
                        Err(e) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key, "exec_status": "3", "tx_hash": null,
                                "error_msg": e.to_string(), "window_id": wid, "transfer_id": *tid,
                            }));
                            fc.fetch_add(1, AO::Relaxed);
                        }
                    }
                    if i < group_len - 1 {
                        let [min_s, max_s] = item_cfg.delay;
                        let secs = if max_s > min_s {
                            rand::thread_rng().gen_range(min_s..=max_s)
                        } else { min_s };
                        if secs > 0 { sleep(Duration::from_secs(secs)).await; }
                    }
                }
            }
        })
    }).collect();

    join_all(tasks).await;

    Ok(BatchTransferSummary {
        total: total_items,
        success: success_count.load(std::sync::atomic::Ordering::Relaxed),
        failed: failed_count.load(std::sync::atomic::Ordering::Relaxed),
        stopped: get_stop_flag(&window_id),
    })
}
