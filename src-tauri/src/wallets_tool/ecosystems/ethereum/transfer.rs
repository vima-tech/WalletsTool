use serde::{Deserialize, Serialize};
use tauri::Emitter;
use alloy_provider::{Provider, RootProvider};
use alloy_transport_http::{Http, Client as AlloyClient};
use alloy::rpc::client::RpcClient;
use alloy::consensus::Transaction as _;
use alloy_primitives::{Address, U256};
use alloy_rpc_types_eth::{TransactionRequest, BlockNumberOrTag};
use alloy_signer_local::{PrivateKeySigner};
use alloy_signer::Signer;
use url::Url;
use std::sync::Arc;
use rand::Rng;
use tokio::sync::Semaphore;
use futures::future::join_all;
use crate::database::get_database_pool;
use crate::wallets_tool::ecosystems::ethereum::provider::{ProviderUtils, create_provider_with_client, create_http_client_with_proxy, AlloyProvider};
use crate::wallets_tool::security::SecureMemory;
use sqlx::Row;
use super::alloy_utils::{parse_ether_to_wei_f64, parse_gwei_to_wei, format_wei_to_ether, format_wei_to_gwei};
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::sync::{Mutex, LazyLock};

// 基于窗口ID的停止标志映射
static STOP_FLAGS: LazyLock<Mutex<HashMap<String, AtomicBool>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

// 辅助函数：获取窗口的停止状态
pub fn get_stop_flag(window_id: &str) -> bool {
    let flags = STOP_FLAGS.lock().unwrap();
    if let Some(flag) = flags.get(window_id) {
        flag.load(Ordering::Relaxed)
    } else {
        false
    }
}

// 辅助函数：设置窗口的停止状态
fn set_stop_flag(window_id: &str, value: bool) {
    let mut flags = STOP_FLAGS.lock().unwrap();
    if let Some(flag) = flags.get(window_id) {
        flag.store(value, Ordering::Relaxed);
    } else {
        flags.insert(window_id.to_string(), AtomicBool::new(value));
    }
}

// 辅助函数：重置窗口的停止状态
pub(crate) fn reset_stop_flag(window_id: &str) {
    set_stop_flag(window_id, false);
}

// 停止转账命令
#[tauri::command]
pub async fn stop_transfer(window_id: String) -> Result<(), String> {
    set_stop_flag(&window_id, true);
    println!("收到停止转账请求，窗口ID: {window_id}");
    Ok(())
}

// 重置停止标志
#[tauri::command]
pub async fn reset_transfer_stop(window_id: String) -> Result<(), String> {
    reset_stop_flag(&window_id);
    println!("重置转账停止标志，窗口ID: {window_id}");
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferConfig {
    pub chain: String,
    pub delay: [u64; 2],
    pub transfer_type: String, // "1", "2", "3", "4"
    pub transfer_amount: f64,
    pub transfer_amount_list: [f64; 2],
    pub left_amount_list: [f64; 2],
    pub amount_precision: u8,
    pub limit_type: String, // "1", "2", "3"
    pub limit_count: u64,
    pub limit_count_list: [u64; 2],
    pub gas_price_type: String, // "1", "2", "3"
    pub gas_price: f64,
    pub gas_price_rate: f64,
    pub max_gas_price: f64,
    pub error_retry: String,
    pub error_count_limit: u32,
    #[serde(default)]
    pub window_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TransferItem {
    pub private_key: SecureMemory,
    pub to_addr: String,
    pub error_msg: String,
    pub retry_flag: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransferResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub error: Option<String>,
}

// RPC提供商信息
#[derive(Debug, Clone)]
pub struct RpcProvider {
    pub rpc_url: String,
    pub priority: i32,
    pub failure_count: i32,
    pub avg_response_time_ms: i32,
}

// RPC节点配置
pub struct RpcConfig {
    pub providers: Vec<RpcProvider>,
    pub chain_id: u64,
}

impl RpcConfig {
    // 基于优先级权重的负载均衡选择RPC
    pub fn get_random_rpc(&self) -> Result<&str, String> {
        if self.providers.is_empty() {
            return Err("没有可用的RPC提供商。请在RPC管理中至少启用一个RPC节点。".to_string());
        }
        
        if self.providers.len() == 1 {
            return Ok(&self.providers[0].rpc_url);
        }
        
        // 计算每个提供商的权重
        let mut weights: Vec<f64> = Vec::new();
        
        for provider in &self.providers {
            // 基础权重：优先级越高（数值越小）权重越大
            let priority_weight = 1.0 / (provider.priority as f64 + 1.0);
            
            // 失败次数惩罚：失败次数越多权重越小
            let failure_penalty = 1.0 / (provider.failure_count as f64 + 1.0);
            
            // 响应时间惩罚：响应时间越长权重越小
            let response_time_penalty = 1.0 / (provider.avg_response_time_ms as f64 + 100.0);
            
            // 综合权重计算
            let weight = priority_weight * failure_penalty * response_time_penalty;
            weights.push(weight);
        }
        
        // 计算权重总和
        let total_weight: f64 = weights.iter().sum();
        
        // 生成随机数进行加权选择
         let mut rng = rand::thread_rng();
         let random_value = rng.gen_range(0.0..total_weight);
        
        let mut cumulative_weight = 0.0;
        for (i, weight) in weights.iter().enumerate() {
            cumulative_weight += weight;
            if random_value <= cumulative_weight {
                return Ok(&self.providers[i].rpc_url);
            }
        }
        
        // 如果由于浮点精度问题没有选中，返回最后一个
        Ok(&self.providers.last().ok_or("RPC提供商列表为空")?.rpc_url)
    }
}

// 从数据库获取RPC配置
pub async fn get_rpc_config(chain: &str) -> Option<RpcConfig> {
    println!("[DEBUG] get_rpc_config - 开始获取链 '{chain}' 的RPC配置");
    
    let pool = get_database_pool();
    
    // 首先获取链信息
    println!("[DEBUG] get_rpc_config - 查询链信息...");
    let chain_info = match sqlx::query(
        r#"
        SELECT id, chain_id FROM chains WHERE chain_key = ?
        "#
    )
    .bind(chain)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => {
            println!("[DEBUG] get_rpc_config - 找到链信息");
            row
        },
        Ok(None) => {
            println!("[ERROR] get_rpc_config - 链未找到: {chain}");
            return None;
        }
        Err(e) => {
            println!("[ERROR] get_rpc_config - 数据库查询链信息失败: {e}");
            return None;
        }
    };
    
    let db_chain_id: i64 = chain_info.get("id");
    let network_chain_id: i64 = chain_info.get("chain_id");
    
    println!("[DEBUG] get_rpc_config - 链信息: db_chain_id={db_chain_id}, network_chain_id={network_chain_id}");
    
    // 获取该链的所有活跃RPC提供商，按优先级排序
    println!("[DEBUG] get_rpc_config - 查询RPC提供商...");
    let rpc_providers = match sqlx::query(
        r#"
        SELECT rpc_url, priority, failure_count, avg_response_time_ms 
        FROM rpc_providers 
        WHERE chain_id = ? AND is_active = 1 
        ORDER BY priority ASC, failure_count ASC, avg_response_time_ms ASC
        "#
    )
    .bind(db_chain_id)
    .fetch_all(&pool)
    .await
    {
        Ok(providers) => {
            println!("[DEBUG] get_rpc_config - 找到 {} 个RPC提供商", providers.len());
            providers
        },
        Err(e) => {
            println!("[ERROR] get_rpc_config - 数据库查询RPC提供商失败: {e}");
            return None;
        }
    };
    
    if rpc_providers.is_empty() {
        println!("[ERROR] get_rpc_config - 没有找到活跃的RPC提供商，链: {chain}。请在RPC管理中至少启用一个RPC节点。");
        return None;
    }
    
    // 构建RPC提供商列表
    let providers: Vec<RpcProvider> = rpc_providers
        .iter()
        .map(|row| {
            let rpc_url = row.get::<String, _>("rpc_url");
            println!("[DEBUG] get_rpc_config - RPC提供商: {rpc_url}");
            RpcProvider {
                rpc_url,
                priority: row.get::<i32, _>("priority"),
                failure_count: row.get::<i32, _>("failure_count"),
                avg_response_time_ms: row.get::<i32, _>("avg_response_time_ms"),
            }
        })
        .collect();
    
    let config = RpcConfig {
        providers,
        chain_id: network_chain_id as u64,
    };
    
    println!("[DEBUG] get_rpc_config - RPC配置创建成功，chain_id: {}", config.chain_id);
    
    Some(config)
}

// RPC请求间隔控制(防止429错误)
use tokio::time::{sleep, Duration};

// 添加随机延迟，防止RPC请求过于密集
async fn add_rpc_delay() {
    // 生成50-150ms随机延迟，避免所有请求同时发送导致RPC限流
    // 移除原来的300-800ms延迟，提升并发性能
    let delay_ms = rand::thread_rng().gen_range(50..150);
    sleep(Duration::from_millis(delay_ms)).await;
}

// 创建Provider(支持代理和请求间隔控制)
pub async fn create_provider(chain: &str, window_id: Option<&str>) -> Result<Arc<AlloyProvider>, Box<dyn std::error::Error>> {
    use crate::wallets_tool::ecosystems::ethereum::proxy_manager::PROXY_MANAGER;
    
    add_rpc_delay().await;
    
    println!("[DEBUG] create_provider - 开始为链 '{chain}' 创建Provider, window_id: {window_id:?}");
    
    let rpc_config = get_rpc_config(chain).await
        .ok_or_else(|| {
            let error_msg = format!("无法获取链 '{chain}' 的RPC配置。可能原因：1) 链不存在  2) 所有RPC节点都已被禁用。请检查RPC管理设置，至少启用一个RPC节点。");
            println!("[ERROR] create_provider - {error_msg}");
            error_msg
        })?;
    
    println!("[DEBUG] create_provider - 获取到RPC配置，chain_id: {}, providers数量: {}", 
             rpc_config.chain_id, rpc_config.providers.len());
    
    let rpc_url = rpc_config.get_random_rpc()
        .map_err(|e| format!("选择RPC失败: {e}"))?;
    println!("[DEBUG] create_provider - 选择的RPC URL: {rpc_url}");
    
    let proxy_url = if let Some(wid) = window_id {
        let config = PROXY_MANAGER.get_config_for_window(wid);
        let using_proxy = config.enabled && !config.proxies.is_empty();
        
        if using_proxy {
            println!("[INFO] 代理已启用，窗口 {} 有 {} 个代理可用", wid, config.proxies.len());
        } else if config.enabled {
            println!("[WARN] 代理已启用但没有配置代理地址，将使用直连模式");
        } else {
            println!("[INFO] 代理未启用，使用直连模式");
        }
        
        PROXY_MANAGER.get_random_proxy_for_window(wid)
    } else {
        let proxy_config = PROXY_MANAGER.get_config();
        let using_proxy = proxy_config.enabled && !proxy_config.proxies.is_empty();
        
        if using_proxy {
            println!("[INFO] 代理已启用，当前有 {} 个代理可用", proxy_config.proxies.len());
        } else if proxy_config.enabled {
            println!("[WARN] 代理已启用但没有配置代理地址，将使用直连模式");
        } else {
            println!("[INFO] 代理未启用，使用直连模式");
        }
        
        PROXY_MANAGER.get_random_proxy()
    };
    
    let provider = create_provider_with_client(rpc_url, proxy_url.as_deref())
        .await
        .map_err(|e| {
            println!("[ERROR] create_provider - Provider创建失败: {e}");
            e
        })?;
    
    println!("[DEBUG] create_provider - Provider创建成功");
    
    match provider.get_chain_id().await {
        Ok(chain_id) => {
            println!("[DEBUG] create_provider - 连接测试成功，链ID: {chain_id}");
        }
        Err(e) => {
            println!("[WARN] create_provider - 连接测试失败: {e}");
        }
    }
    
Ok(Arc::new(provider))
}

pub async fn create_signer_provider(
    chain: &str,
    window_id: Option<&str>,
    signer: &PrivateKeySigner,
) -> Result<impl Provider, Box<dyn std::error::Error>> {
    use crate::wallets_tool::ecosystems::ethereum::proxy_manager::PROXY_MANAGER;
    use alloy_provider::ProviderBuilder;
    
    let rpc_config = get_rpc_config(chain).await
        .ok_or_else(|| {
            let error_msg = format!("无法获取链 '{chain}' 的RPC配置");
            error_msg
        })?;
    
    let rpc_url = rpc_config.get_random_rpc()
        .map_err(|e| format!("选择RPC失败: {e}"))?;
    
    let proxy_url = if let Some(wid) = window_id {
        PROXY_MANAGER.get_random_proxy_for_window(wid)
    } else {
        PROXY_MANAGER.get_random_proxy()
    };
    
    let http_client = create_http_client_with_proxy(proxy_url.as_deref()).await?;
    let http_client: AlloyClient = http_client;
    let url: Url = rpc_url.parse()
        .map_err(|e| format!("RPC URL 解析失败: {e}"))?;
    
    let http = Http::with_client(http_client, url);
    let rpc_client = RpcClient::new(http, false);
    let root = RootProvider::new(rpc_client);

    let provider = ProviderBuilder::new()
        .wallet(signer.clone())
        .connect_provider(root);
    
    Ok(provider)
}

/// 根据链ID和用户配置获取缓冲参数
/// 返回: (gas_price_buffer_factor, gas_limit_buffer_factor, min_gas_limit)
/// - gas_price_buffer_factor: Gas Price缓冲百分比（如150表示1.5倍）
/// - gas_limit_buffer_factor: Gas Limit缓冲百分比（如110表示1.1倍）
/// - min_gas_limit: 最小Gas Limit值
fn get_chain_buffer_params_with_user_config(chain_id: u64, config: &TransferConfig) -> (u32, u32, u64) {
    // 获取链的基础参数
    let (base_price_buffer, base_limit_buffer, min_gas_limit, _) = get_chain_base_params(chain_id);

    // 根据用户配置的 limit_type 调整缓冲
    // 如果用户使用固定/随机 Gas Limit，说明用户对 Gas Limit 有明确控制，可以减少缓冲
    let (gas_price_buffer_factor, gas_limit_buffer_factor) = match config.limit_type.as_str() {
        "2" | "3" => {
            // 固定/随机 Gas Limit 模式 - 用户指定了具体值，使用更小的缓冲
            (base_price_buffer, 105) // 仅 5% 缓冲
        }
        _ => {
            // 自动估算模式 - 使用链基础缓冲
            (base_price_buffer, base_limit_buffer)
        }
    };

    (gas_price_buffer_factor, gas_limit_buffer_factor, min_gas_limit)
}

/// 获取链的基础参数（不涉及用户配置）
fn get_chain_base_params(chain_id: u64) -> (u32, u32, u64, bool) {
    match chain_id {
        // Ethereum Mainnet & Testnets (EIP-1559)
        1 | 5 | 11155111 => (150, 110, 21000, true),

        // Polygon (EIP-1559)
        137 | 80001 => (150, 110, 21000, true),

        // Arbitrum (EIP-1559)
        42161 | 421613 => (150, 115, 25000, true),

        // Optimism (EIP-1559)
        10 | 420 => (150, 115, 25000, true),

        // BSC (非EIP-1559，传统Gas Price)
        56 | 97 => (150, 110, 21000, false),

        // Avalanche (EIP-1559)
        43114 | 43113 => (150, 110, 21000, true),

        // Base (EIP-1559)
        8453 | 84531 => (150, 110, 21000, true),

        // Linea (EIP-1559)
        59144 | 59140 => (150, 110, 21000, true),

        // zkSync Era (EIP-1559)
        324 | 280 => (150, 120, 30000, true),

        // Mantle (EIP-1559)
        5000 | 5001 => (150, 115, 25000, true),

        // Metis (非EIP-1559)
        1088 | 599 => (150, 115, 25000, false),

        // 其他链使用默认保守参数
        _ => (160, 120, 25000, true),
    }
}

/// 检查 estimate_gas 返回值是否合理
/// 简单 ETH 转账的实际 gas 应该在合理范围内
fn validate_estimated_gas(estimated: U256, chain_id: u64, default_min: u64) -> U256 {
    // 简单 ETH 转账（EOA -> EOA，无 data）的理论最小值
    const THEORETICAL_MIN: u64 = 21000;
    // 异常上限：100万 gas（任何简单转账都不应该需要这么多）
    const MAX_REASONABLE: u64 = 1_000_000;

    // 如果估计值低于理论最小值，使用理论最小值
    if estimated < U256::from(THEORETICAL_MIN) {
        println!("[WARN] 链{chain_id}的estimate_gas返回{estimated}，低于理论最小值{THEORETICAL_MIN}，使用默认值");
        return U256::from(default_min);
    }

    // 如果估计值超过上限，认为是异常值
    if estimated > U256::from(MAX_REASONABLE) {
        println!("[WARN] 链{chain_id}的estimate_gas返回{estimated}，超过上限{MAX_REASONABLE}，使用默认值");
        return U256::from(default_min);
    }

    estimated
}

// 转账工具函数
pub struct TransferUtils;

impl TransferUtils {
    // 获取当前网络的baseFee
    pub async fn get_base_fee(
        provider: Arc<AlloyProvider>,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        // 获取最新区块
        let latest_block = provider.get_block(alloy_rpc_types_eth::BlockId::Number(BlockNumberOrTag::Latest)).await?;
        
        if let Some(block) = latest_block {
            if let Some(base_fee) = block.header.base_fee_per_gas {
                println!("[DEBUG] 获取到当前baseFee: {} wei ({} gwei)", 
                    base_fee, 
                    format_wei_to_gwei(U256::from(base_fee))
                );
                return Ok(U256::from(base_fee));
            }
        }
        
        // 如果无法获取baseFee，返回默认值（适用于非EIP-1559网络）
        println!("[DEBUG] 无法获取baseFee，使用默认值0");
        Ok(U256::from(0))
    }

    // 获取区块Gas Limit
    pub async fn get_block_gas_limit(
        provider: Arc<AlloyProvider>,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        match provider.get_block(BlockNumberOrTag::Latest.into()).await {
            Ok(Some(block)) => {
                let raw_gas_limit = U256::from(block.header.gas_limit);
                println!("[DEBUG] 从RPC获取到的原始区块gas limit: {raw_gas_limit}");
                
                // 合理性检查：如果gas limit超过1亿，认为是异常值
                let max_reasonable_gas_limit = U256::from(100_000_000u64); // 1亿
                
                if raw_gas_limit > max_reasonable_gas_limit {
                    println!("[WARN] 检测到异常的区块gas limit: {raw_gas_limit}，远超合理范围");
                    
                    // 根据链ID返回合理的默认值
                    let chain_id = (provider.get_chain_id().await).unwrap_or_default();
                    
                    let default_gas_limit = match chain_id {
                        42161 => U256::from(30_000_000u64), // Arbitrum One
                        1 => U256::from(30_000_000u64),     // Ethereum Mainnet
                        137 => U256::from(30_000_000u64),   // Polygon
                        56 => U256::from(140_000_000u64),   // BSC (更高的gas limit)
                        _ => U256::from(30_000_000u64),     // 其他链的默认值
                    };
                    
                    println!("[INFO] 使用链ID {chain_id} 的默认gas limit: {default_gas_limit}");
                    Ok(default_gas_limit)
                } else {
                    println!("[DEBUG] 区块gas limit正常: {raw_gas_limit}");
                    Ok(raw_gas_limit)
                }
            }
            Ok(None) => {
                eprintln!("[ERROR] 无法获取最新区块信息");
                Err("无法获取最新区块信息".into())
            }
            Err(e) => {
                eprintln!("[ERROR] 获取区块gas limit失败: {e}");
                Err(format!("获取区块gas limit失败: {e}").into())
            }
        }
    }

    // 预检查余额是否充足（在实际转账前进行检查，避免RPC调用后才发现余额不足）
    pub async fn pre_check_balance(
        config: &TransferConfig,
        provider: Arc<AlloyProvider>,
        wallet_address: Address,
        to_address: Address,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 获取当前余额
        let balance = match provider.get_balance(wallet_address).await {
            Ok(balance) => balance,
            Err(e) => {
                // 获取当前使用的RPC URL
                let error_msg = e.to_string();
                let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
                    match rpc_config.get_random_rpc() {
                        Ok(url) => url.to_string(),
                        Err(e) => format!("获取RPC地址失败: {e}")
                    }
                } else {
                    "未知RPC".to_string()
                };
                return Err(format!("获取钱包余额失败 (RPC: {rpc_url}): {error_msg}").into());
            }
        };
        
        if balance.is_zero() {
            return Err("当前余额为0，无法进行转账操作！".into());
        }
        
        // 获取Gas Price进行预估
        let gas_price = Self::get_gas_price(config, provider.clone()).await?;
        
        if gas_price.is_zero() {
            return Err("获取到的 gas price 为0，请检查网络连接或RPC配置".into());
        }
        // 根据转账类型进行不同的余额检查
        match config.transfer_type.as_str() {
            "1" => {
                // 全部转账 - 需要预留Gas费用
                let estimated_gas_limit = Self::get_gas_limit(config, provider.clone(), wallet_address, to_address, parse_ether_to_wei_f64(0.000003)?).await?;
          
                print!("estimated_gas_limit: {estimated_gas_limit}");
                let estimated_gas_fee = gas_price * estimated_gas_limit;
                if estimated_gas_fee >= balance {
                    return Err(format!(
                        "余额不足支付Gas费用！当前余额: {} ETH，预估Gas费用: {} ETH",
                        format_wei_to_ether(balance),
                        format_wei_to_ether(estimated_gas_fee)
                    ).into());
                }
            }
            "2" => {
                // 转账固定数量
                let transfer_amount = parse_ether_to_wei_f64(config.transfer_amount)?;
                let estimated_gas_limit = Self::get_gas_limit(config, provider.clone(), wallet_address, to_address, parse_ether_to_wei_f64(0.000003)?).await?;
                let estimated_gas_fee = gas_price * estimated_gas_limit;
                let total_needed = transfer_amount + estimated_gas_fee;
                print!("estimated_gas_limit: {estimated_gas_limit}");
                if total_needed > balance {
                    return Err(format!(
                        "余额不足！需要: {} ETH (转账: {} + Gas: {} ETH)，当前余额: {} ETH",
format_wei_to_ether(total_needed),
                        format_wei_to_ether(transfer_amount),
                        format_wei_to_ether(estimated_gas_fee),
                        format_wei_to_ether(balance)
                    ).into());
                }
            }
            "3" => {
                // 转账随机数量 - 使用最大可能金额进行检查
                let max_transfer_amount = parse_ether_to_wei_f64(config.transfer_amount_list[1])?;
                let estimated_gas_limit =Self::get_gas_limit(config, provider.clone(), wallet_address, to_address, parse_ether_to_wei_f64(0.000003)?).await?;
                let estimated_gas_fee = gas_price * estimated_gas_limit;
                let total_needed = max_transfer_amount + estimated_gas_fee;
                
                if total_needed > balance {
                    return Err(format!(
                        "余额不足！最大可能需要: {} ETH (转账: {} + Gas: {} ETH)，当前余额: {} ETH",
                        format_wei_to_ether(total_needed),
                        format_wei_to_ether(max_transfer_amount),
                        format_wei_to_ether(estimated_gas_fee),
                        format_wei_to_ether(balance)
                    ).into());
                }
            }
            "4" => {
                // 剩余随机数量 - 检查是否有足够余额满足最小剩余要求
                 let estimated_gas_limit = match config.limit_type.as_str() {
                    "1" => {
                        // 自动估算模式，使用最小转账金额进行估算
                        let min_transfer = parse_ether_to_wei_f64(0.000003)?;
                        Self::get_gas_limit(config, provider.clone(), wallet_address, to_address, min_transfer).await?
                    }
                    "2" => U256::from(config.limit_count),
                    "3" => {
                        // 使用最大可能的gas limit进行保守估算
                        U256::from(config.limit_count_list[1])
                    }
                    _ => U256::from(21000), // 默认ETH转账gas limit
                };
                let estimated_gas_fee = gas_price * estimated_gas_limit;
                let balance_ether = format_wei_to_ether(balance).parse::<f64>()?;
                let gas_fee_ether = format_wei_to_ether(estimated_gas_fee).parse::<f64>()?;
                let available_balance = balance_ether - gas_fee_ether;
                
                if available_balance <= config.left_amount_list[1] {
                    return Err(format!(
                        "余额不足！可用余额: {} ETH (总余额: {} - Gas: {} ETH)，无法满足最大剩余数量 {} ETH 要求",
                        available_balance, balance_ether, gas_fee_ether, config.left_amount_list[1]
                    ).into());
                }
            }
            _ => return Err("无效的转账类型".into()),
        }
        
        Ok(())
    }

    // 获取Gas Price
    pub async fn get_gas_price(
        config: &TransferConfig,
        provider: Arc<AlloyProvider>,
    ) -> Result<U256, Box<dyn std::error::Error>> {
        // 获取当前网络的baseFee
        let base_fee = if let Ok(fee) = Self::get_base_fee(provider.clone()).await {
            fee
        } else {
            // 获取当前使用的RPC URL
            let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
                match rpc_config.get_random_rpc() {
                    Ok(url) => url.to_string(),
                    Err(e) => format!("获取RPC地址失败: {e}")
                }
            } else {
                "未知RPC".to_string()
            };
            println!("[WARN] 获取Base Fee失败 (RPC: {rpc_url}), 使用默认值0");
            U256::from(0)
        };
        
        // 获取链ID并判断链类型
        let chain_id = provider.get_chain_id().await?;
        let chain_id_u64 = chain_id;
        let is_arbitrum = chain_id_u64 == 42161; // Arbitrum One
        
        // 判断是否为真正的EIP-1559链
        // BSC(56)虽然可能返回baseFee，但实际上不是标准的EIP-1559链，其Gas Price机制不同
        let is_eip1559_chain = match chain_id_u64 {
            1 => true,      // Ethereum Mainnet
            5 => true,      // Goerli
            11155111 => true, // Sepolia
            137 => true,    // Polygon
            42161 => true,  // Arbitrum One
            10 => true,     // Optimism
            56 => false,    // BSC - 非EIP-1559链
            97 => false,    // BSC Testnet - 非EIP-1559链
            _ => base_fee > U256::from(0), // 其他链根据是否有baseFee判断
        };
        
        println!("[DEBUG] 链ID: {chain_id_u64}, 是否为Arbitrum: {is_arbitrum}, 是否为EIP-1559链: {is_eip1559_chain}");
        
        let calculated_gas_price = match config.gas_price_type.as_str() {
            "1" => {
                // 使用网络Gas Price
                let gas_price = match provider.get_gas_price().await {
                    Ok(price) => U256::from(price),
                    Err(e) => {
                        // 获取当前使用的RPC URL
                        let error_msg = e.to_string();
                        let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
                            match rpc_config.get_random_rpc() {
                                Ok(url) => url.to_string(),
                                Err(e) => format!("获取RPC地址失败: {e}")
                            }
                        } else {
                            "未知RPC".to_string()
                        };
                        return Err(format!("获取网络Gas Price失败 (RPC: {rpc_url}): {error_msg}").into());
                    }
                };
                
                // 检查最大Gas Price限制
                if config.max_gas_price > 0.0 {
                    let gas_price_gwei = format_wei_to_gwei(gas_price).parse::<f64>()?;
                    if gas_price_gwei > config.max_gas_price {
                        return Err("base gas price 超出最大值限制".into());
                    }
                }
                
                gas_price
            }
            "2" => {
                // 使用固定Gas Price
                parse_gwei_to_wei(config.gas_price)
            }
            "3" => {
                // 使用溢价Gas Price
                let base_gas_price = match provider.get_gas_price().await {
                    Ok(price) => U256::from(price),
                    Err(e) => {
                        // 获取当前使用的RPC URL
                        let error_msg = e.to_string();
                        let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
                            match rpc_config.get_random_rpc() {
                                Ok(url) => url.to_string(),
                                Err(e) => format!("获取RPC地址失败: {e}")
                            }
                        } else {
                            "未知RPC".to_string()
                        };
                        return Err(format!("获取基础Gas Price失败 (RPC: {rpc_url}): {error_msg}").into());
                    }
                };
                
                // 安全地计算gas price rate，避免溢出
                let rate_percentage = config.gas_price_rate * 100.0;
                if !(0.0..=f64::MAX / 2.0).contains(&rate_percentage) {
                    println!("[ERROR] Gas price rate 值异常: {rate_percentage}");
                    return Err(format!("Gas price rate 值异常: {rate_percentage}").into());
                }
                
                // 使用U256进行安全计算，避免u64溢出
                let rate_u256 = U256::from(rate_percentage as u64);
                let multiplier = U256::from(100) + rate_u256;
                let gas_price_with_rate = base_gas_price * multiplier / U256::from(100);
                
                // 检查最大Gas Price限制
                if config.max_gas_price > 0.0 {
                    let base_gas_price_gwei = format_wei_to_gwei(base_gas_price).parse::<f64>()?;
                    if base_gas_price_gwei > config.max_gas_price {
                        return Err("base gas price 超出最大值限制".into());
                    }
                    
                    let final_gas_price_gwei = format_wei_to_gwei(gas_price_with_rate).parse::<f64>()?;
                    if final_gas_price_gwei >= config.max_gas_price {
                        return Ok(parse_gwei_to_wei(config.max_gas_price));
                    }
                }
                
                gas_price_with_rate
            }
            _ => return Err("gas price type error".into()),
        };
        
        // 对于BSC链，使用更合理的Gas Price计算方式
        if chain_id_u64 == 56 || chain_id_u64 == 97 {
            // BSC链的特殊处理：直接返回计算的Gas Price，不使用baseFee
            println!("[DEBUG] BSC链特殊处理，直接使用计算的Gas Price: {} gwei", 
                format_wei_to_gwei(calculated_gas_price)
            );
            return Ok(calculated_gas_price);
        }
        
        // 确保Gas Price高于baseFee（仅对真正的EIP-1559链生效）
        if is_eip1559_chain && base_fee > U256::from(0) {
            let min_gas_price = if is_arbitrum {
                // Arbitrum链：baseFee * 1.5 (50%安全边际)
                base_fee * U256::from(150) / U256::from(100)
            } else {
                // 其他EIP-1559链：baseFee * 1.2 (20%安全边际)
                base_fee * U256::from(120) / U256::from(100)
            };
            
            let final_gas_price = if calculated_gas_price < min_gas_price {
                println!("[DEBUG] 计算的Gas Price ({} gwei) 低于最小要求 ({} gwei)，使用最小值", 
                    format_wei_to_gwei(calculated_gas_price),
                    format_wei_to_gwei(min_gas_price)
                );
                min_gas_price
            } else {
                calculated_gas_price
            };
            
            println!("[DEBUG] 最终Gas Price: {} gwei (baseFee: {} gwei)", 
                format_wei_to_gwei(final_gas_price),
                format_wei_to_gwei(base_fee)
            );
            
            Ok(final_gas_price)
        } else {
            // 非EIP-1559网络（如BSC），直接使用计算的Gas Price，不受baseFee影响
            println!("[DEBUG] 非EIP-1559网络或无baseFee，使用计算的Gas Price: {} gwei", 
                format_wei_to_gwei(calculated_gas_price)
            );
            Ok(calculated_gas_price)
        }
    }

    // 获取Gas Limit
    pub async fn get_gas_limit(
        config: &TransferConfig,
        provider: Arc<AlloyProvider>,
        from: Address,
        to: Address,
        value: U256,
    ) -> Result<U256, String> {
        // 根据链配置的原生货币符号判断是否为ETH转账
        let chain_config = ProviderUtils::get_chain_config(&config.chain).await
            .map_err(|e| format!("获取链配置失败: {e}"))?;
        let is_eth = chain_config.currency_symbol == "ETH";
        Self::get_gas_limit_with_token_type(config, provider, from, to, value, is_eth).await
    }

    // 获取Gas Limit（支持区分代币类型）
    pub async fn get_gas_limit_with_token_type(
        config: &TransferConfig,
        provider: Arc<AlloyProvider>,
        from: Address,
        to: Address,
        value: U256,
        is_eth: bool,
    ) -> Result<U256, String> {
        let tx = TransactionRequest {
            from: Some(from),
            to: Some(to.into()),
            value: Some(value),
            ..Default::default()
        };
        Self::get_gas_limit_for_tx(config, provider, tx, is_eth).await
    }

    pub async fn get_gas_limit_for_tx(
        config: &TransferConfig,
        provider: Arc<AlloyProvider>,
        tx: TransactionRequest,
        is_eth: bool,
    ) -> Result<U256, String> {
        let block_gas_limit = match Self::get_block_gas_limit(provider.clone()).await {
            Ok(limit) => {
                println!("[DEBUG] 获取到区块gas limit: {limit}");
                limit
            }
            Err(e) => {
                println!("[WARN] 获取区块gas limit失败: {e}, 使用默认上限");
                U256::from(30_000_000)
            }
        };

        let max_safe_gas_limit = block_gas_limit * U256::from(80) / U256::from(100);
        println!("[DEBUG] 区块gas limit安全上限(80%): {max_safe_gas_limit}");

        match config.limit_type.as_str() {
            "1" => {
                // 自动估算Gas Limit
                let estimated_gas = match provider.estimate_gas(tx).await {
                    Ok(gas) => {
                        let gas = U256::from(gas);
                        println!("[DEBUG] estimate_gas成功: {gas}");
                        gas
                    }
                    Err(e) => {
                        // 获取当前使用的RPC URL
                        let error_msg = e.to_string();
                        let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
                            match rpc_config.get_random_rpc() {
                                Ok(url) => url.to_string(),
                                Err(e) => format!("获取RPC地址失败: {e}")
                            }
                        } else {
                            "未知RPC".to_string()
                        };
                        println!("[WARN] estimate_gas失败 (RPC: {rpc_url}): {error_msg}, 使用默认值");
                        // 直接使用默认值，不再尝试获取平均gas limit（因为tx.gas是gas limit而非实际使用值）
                        {
                            println!("[WARN] 获取平均gas limit也失败，使用固定默认值");
                            // 直接使用合理的固定默认值，不使用区块gas limit计算
                            if is_eth {
                                // ETH转账
                                U256::from(21_000)
                            } else {
                                // 代币转账根据链类型设置默认值
                                let chain_id = provider.get_chain_id().await.unwrap_or_default();
                                let default_token_gas = match chain_id {
                                    42161 => U256::from(150_000),  // Arbitrum One - 更高的默认值
                                    1 => U256::from(65_000),       // Ethereum Mainnet
                                    137 => U256::from(65_000),     // Polygon
                                    56 => U256::from(60_000),      // BSC
                                    _ => U256::from(80_000),       // 其他链使用保守值
                                };
                                println!("[DEBUG] 链ID: {chain_id}, 代币转账默认Gas Limit: {default_token_gas}");
                                default_token_gas
                            }
                        }
                    }
                };
                
                // 添加合理性检查：根据代币类型区分处理
                let mut gas_limit = if is_eth {
                    // ETH转账的gas limit处理
                    if estimated_gas < U256::from(21_000) {
                        // ETH转账最小值为21000
                        println!("[DEBUG] ETH转账gas limit过小，使用最小值21000");
                        U256::from(21_000)
                    } else {
                        // 为估算值添加5%的安全边际
                        let gas_with_margin = estimated_gas * U256::from(105) / U256::from(100);
                        println!("[DEBUG] ETH转账添加5%安全边际: {estimated_gas} -> {gas_with_margin}");
                        gas_with_margin
                    }
                } else {
                    // 代币转账的gas limit处理
                    let chain_id = provider.get_chain_id().await.unwrap_or_default();
                    let min_token_gas = match chain_id {
                        42161 => U256::from(80_000),  // Arbitrum One - 更高的最小值
                        1 => U256::from(60_000),      // Ethereum Mainnet
                        137 => U256::from(60_000),    // Polygon
                        56 => U256::from(60_000),     // BSC
                        _ => U256::from(80_000),      // 其他链使用保守值
                    };
                    println!("[DEBUG] 链ID: {chain_id}, 代币转账最小Gas Limit: {min_token_gas}");
                    
                    if estimated_gas < min_token_gas {
                        // 代币转账gas limit过小，直接使用最小值
                        println!("[DEBUG] 估算的gas limit {estimated_gas} 小于最小值 {min_token_gas}，使用最小值");
                        min_token_gas
                    } else {
                        // 为估算值添加20%的安全边际，但确保不低于最小值
                        let gas_with_margin = estimated_gas * U256::from(120) / U256::from(100);
                        let final_gas = std::cmp::max(gas_with_margin, min_token_gas);
                        println!("[DEBUG] 代币转账添加20%安全边际: {estimated_gas} -> {gas_with_margin} (最终: {final_gas})");
                        final_gas
                    }
                };
                
                // 关键检查：确保gas limit不超过区块限制
                if gas_limit > max_safe_gas_limit {
                    println!("[WARN] 估算的gas limit {gas_limit} 超过区块安全上限 {max_safe_gas_limit}，调整为安全上限");
                    gas_limit = max_safe_gas_limit;
                }
                
                // 最终验证：确保gas limit在合理范围内
                if gas_limit > block_gas_limit {
                    println!("[ERROR] gas limit {gas_limit} 仍然超过区块限制 {block_gas_limit}，强制设为区块限制的70%");
                    gas_limit = block_gas_limit * U256::from(70) / U256::from(100);
                }
                
                println!("[INFO] 最终确定的gas limit: {gas_limit}");
                Ok(gas_limit)
            }
            "2" => {
                // 使用固定Gas Limit，但仍需检查是否超过区块限制
                let fixed_gas_limit = U256::from(config.limit_count);
                if fixed_gas_limit > max_safe_gas_limit {
                    println!("[WARN] 固定gas limit {fixed_gas_limit} 超过区块安全上限 {max_safe_gas_limit}，调整为安全上限");
                    Ok(max_safe_gas_limit)
                } else {
                    Ok(fixed_gas_limit)
                }
            }
            "3" => {
                // 使用随机Gas Limit，但仍需检查是否超过区块限制
                let mut rng = rand::thread_rng();
                let random_gas_limit = rng.gen_range(config.limit_count_list[0]..=config.limit_count_list[1]);
                let gas_limit = U256::from(random_gas_limit);
                
                if gas_limit > max_safe_gas_limit {
                    println!("[WARN] 随机gas limit {gas_limit} 超过区块安全上限 {max_safe_gas_limit}，调整为安全上限");
                    Ok(max_safe_gas_limit)
                } else {
                    Ok(gas_limit)
                }
            }
            _ => Err("gas limit type error".to_string()),
        }
    }
}

// Tauri命令：基础币转账
#[tauri::command]
pub async fn base_coin_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    item: TransferItem,
    config: TransferConfig,
) -> Result<TransferResult, String> {
    match base_coin_transfer_internal(app_handle, index, item, config).await {
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

// 内部基础币转账实现
async fn base_coin_transfer_internal<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    mut item: TransferItem,
    config: TransferConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    item.retry_flag = false;
    
    // 0. 获取窗口ID并检查停止状态
    let window_id = config.window_id.as_deref().unwrap_or("");
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }
    
    // 不再在方法开头创建固定的provider，改为在每次RPC调用时动态获取
    
    // 创建钱包
    let wallet = item.private_key.use_secret(|pk| {
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
    .map_err(|e| format!("私钥格式错误: {e}，请检查私钥格式是否正确（应为64位十六进制字符串，可带或不带0x前缀）"))?;
    // 优先通过统一ProviderUtils获取链ID，避免重复查询逻辑
    let chain_id = match ProviderUtils::get_chain_id(&config.chain).await {
        Ok(id) => id,
        Err(_) => {
            // 如果ProviderUtils获取失败，尝试从RPC配置获取
            match get_rpc_config(&config.chain).await {
                Some(c) => c.chain_id,
                None => {
                    return Err(format!(
                        "无法获取链 '{}' 的配置信息。请检查：1) 链是否存在  2) 是否至少有一个启用的RPC节点。",
                        config.chain
                    ).into());
                }
            }
        }
    };
    let wallet = wallet.with_chain_id(Some(chain_id));
    let wallet_address = wallet.address();
    
    // 解析目标地址
    if item.to_addr.trim().is_empty() {
        return Err("目标地址不能为空，请先导入接收地址！".into());
    }
    let to_address: Address = item.to_addr.parse().map_err(|e| {
        format!("目标地址格式错误: {e}，请检查地址格式是否正确")
    })?;
    
    // 获取当前使用的RPC URL用于错误信息
    let rpc_url = if let Some(rpc_config) = get_rpc_config(&config.chain).await {
        match rpc_config.get_random_rpc() {
            Ok(url) => url.to_string(),
            Err(e) => {
                return Err(format!("获取RPC地址失败: {e}").into());
            }
        }
    } else {
        return Err(format!(
            "无法获取链 '{}' 的RPC配置。请在RPC管理中至少启用一个RPC节点。",
            config.chain
        ).into());
    };
    
    // 预检查余额是否充足（避免RPC调用后才发现余额不足）
    let provider_for_precheck = create_provider(&config.chain, config.window_id.as_deref()).await.map_err(|e| {
        format!("获取RPC提供商失败: {e}")
    })?;
    TransferUtils::pre_check_balance(&config, provider_for_precheck.clone(), wallet_address, to_address).await.map_err(|e| {
        format!("{e}: (RPC: {rpc_url}) 余额预检查失败")
    })?;
    
    // 获取余额
    let provider_for_balance = create_provider(&config.chain, config.window_id.as_deref()).await.map_err(|e| {
        format!("获取RPC提供商失败: {e}")
    })?;
    let balance = provider_for_balance.get_balance(wallet_address).await.map_err(|e| {
        format!("获取余额失败 (RPC: {rpc_url}): {e}")
    })?;
    let balance_ether_str = format_wei_to_ether(balance);
    
    println!("序号：{index}, 当前余额为: {balance_ether_str} ETH");
    
    // 获取Gas Price
    let provider_for_gas_price = create_provider(&config.chain, config.window_id.as_deref()).await.map_err(|e| {
        format!("获取RPC提供商失败: {e}")
    })?;
    
    let gas_price = TransferUtils::get_gas_price(&config, provider_for_gas_price.clone()).await.map_err(|e| {
        format!("获取Gas Price失败 (RPC: {rpc_url}): {e}")
    })?;
    

    
    // 检查gas_price是否为0
    if gas_price.is_zero() {
        return Err("获取到的 gas price 为0，请检查网络连接或RPC配置".into());
    }
    
    // 获取Gas Limit - 根据用户设置直接获取，避免不必要的网络调用
    let mut gas_limit = match config.limit_type.as_str() {
        "1" => {
            // 自动估算模式才需要网络调用
            // 对于全部转账，需要使用实际的转账金额来估算gas limit
            if config.transfer_type == "1" {
                // 全部转账：使用多层回退机制估算gas limit
                // 1. 首先尝试用余额的90%估算
                let amount_90_percent = balance * U256::from(90) / U256::from(100);
                let provider_for_gas_limit_90 = create_provider(&config.chain, config.window_id.as_deref()).await?;
                match TransferUtils::get_gas_limit(
                     &config,
                     provider_for_gas_limit_90.clone(),
                     wallet_address,
                     to_address,
                     amount_90_percent,
                 ).await {
                     Ok(gas_limit) => gas_limit,
                     Err(_) => {
                         println!("序号：{index}, 90%余额估算gas limit失败，尝试0.001 ETH估算");
                         // 2. 如果90%估算失败，尝试用0.001 ETH估算
                         let fallback_amount = parse_ether_to_wei_f64(0.000003).map_err(|e| format!("解析金额失败: {e}"))?;
                         let provider_for_gas_limit_fallback = create_provider(&config.chain, config.window_id.as_deref()).await?;
                         match TransferUtils::get_gas_limit(
                             &config,
                             provider_for_gas_limit_fallback.clone(),
                             wallet_address,
                             to_address,
                             fallback_amount,
                         ).await {
                             Ok(gas_limit) => gas_limit,
                             Err(_) => {
                                 println!("序号：{index}, 0.000003 ETH估算gas limit也失败");
                                 // 3. 如果0.001 ETH也失败，返回余额不足错误
                                 return Err("当前余额不足支付Gas费用，不做转账操作！".into());
                             }
                         }
                     }
                 }
            } else {
                // 其他转账类型使用最小金额估算
                let estimate_amount = parse_ether_to_wei_f64(0.000003).map_err(|e| format!("解析金额失败: {e}"))?;
                let provider_for_gas_limit_estimate = create_provider(&config.chain, config.window_id.as_deref()).await?;
                TransferUtils::get_gas_limit(
                    &config,
                    provider_for_gas_limit_estimate.clone(),
                    wallet_address,
                    to_address,
                    estimate_amount,
                ).await.map_err(|e| format!("获取gas limit失败: {e}"))?
            }
        }
        "2" => {
            // 固定数量模式直接使用设定值
            U256::from(config.limit_count)
        }
        "3" => {
            // 随机范围模式生成随机值
            let mut rng = rand::thread_rng();
            let random_limit = rng.gen_range(config.limit_count_list[0]..=config.limit_count_list[1]);
            U256::from(random_limit)
        }
        _ => {
            return Err("gas limit type error".into());
        }
    };
    
    println!("序号：{index}, gas limit: {gas_limit}");

    // 计算转账金额
    let transfer_amount = match config.transfer_type.as_str() {
        "1" => {
            // 全部转账 - 一次性计算最大可转走金额
            // 核心公式: max_transfer = balance - reserved_gas
            // 关键：根据用户配置和链类型设置差异化缓冲策略

            // 发送状态到前端
            let _ = app_handle.emit("transfer_status_update", serde_json::json!({
                "index": index - 1,
                "error_msg": "计算最大转账金额...",
                "exec_status": "1"
            }));

            // 获取链ID用于确定链类型
            let chain_id = (ProviderUtils::get_chain_id(&config.chain).await).unwrap_or(1);

            // 根据链类型和用户配置获取缓冲参数
            // 如果用户使用固定 Gas Price/Gas Limit，缓冲可以小一些
            let (gas_price_buffer_factor, gas_limit_buffer_factor, min_gas_limit) =
                get_chain_buffer_params_with_user_config(chain_id, &config);

            println!("序号：{}, 链ID={}, GasPrice缓冲={}%, GasLimit缓冲={}%, 最小GasLimit={}, 用户GasPriceType={}, 用户LimitType={}",
                index, chain_id, gas_price_buffer_factor, gas_limit_buffer_factor, min_gas_limit, config.gas_price_type, config.limit_type);

            // 计算Gas Price缓冲
            let gas_price_buffer = gas_price * U256::from(gas_price_buffer_factor) / U256::from(100);

            // 估算Gas Limit
            // 根据用户配置的 limit_type 决定如何处理
            let final_gas_limit: U256 = match config.limit_type.as_str() {
                "1" => {
                    // 自动估算模式
                    let provider = create_provider(&config.chain, config.window_id.as_deref()).await?;
                    let estimate_amount = balance * U256::from(90) / U256::from(100);
                    let estimated_gas_limit = TransferUtils::get_gas_limit(
                        &config,
                        provider.clone(),
                        wallet_address,
                        to_address,
                        estimate_amount,
                    ).await.unwrap_or(U256::from(min_gas_limit));

                    // 验证 estimate_gas 返回值是否合理
                    let validated_gas = validate_estimated_gas(estimated_gas_limit, chain_id, min_gas_limit);
                    println!("序号：{index}, estimate_gas原始值={estimated_gas_limit}, 验证后={validated_gas}");

                    // 添加链特定的缓冲（应对Gas Used波动）
                    let gas_limit_with_buffer = validated_gas * U256::from(gas_limit_buffer_factor) / U256::from(100);
                    std::cmp::max(gas_limit_with_buffer, U256::from(min_gas_limit))
                }
                "2" => {
                    // 固定Gas Limit模式 - 直接使用用户配置值
                    // 添加小量缓冲（5%）以应对可能的Gas Used波动
                    let user_gas_limit = U256::from(config.limit_count);
                    let gas_limit_with_buffer = user_gas_limit * U256::from(105) / U256::from(100);
                    std::cmp::max(gas_limit_with_buffer, U256::from(min_gas_limit))
                }
                "3" => {
                    // 随机Gas Limit模式 - 使用用户配置范围的平均值
                    let avg_limit = (config.limit_count_list[0] + config.limit_count_list[1]) / 2;
                    let gas_limit_with_buffer = U256::from(avg_limit) * U256::from(105) / U256::from(100);
                    std::cmp::max(gas_limit_with_buffer, U256::from(min_gas_limit))
                }
                _ => U256::from(min_gas_limit),
            };

            // 计算预留的Gas费用
            let reserved_gas_fee = gas_price_buffer * final_gas_limit;

            // 直接计算最大可转走金额
            if reserved_gas_fee >= balance {
                return Err(format!(
                    "余额不足支付Gas费用！当前余额: {} ETH，预留Gas: {} ETH",
                    format_wei_to_ether(balance),
                    format_wei_to_ether(reserved_gas_fee)
                ).into());
            }

            let max_transfer_amount = balance - reserved_gas_fee;
            let max_transfer_eth = format_wei_to_ether(max_transfer_amount);

            println!("序号：{index}, 全部转账: balance={balance}, max_transfer={max_transfer_eth} ETH, reserved_gas={reserved_gas_fee} WEI, gas_limit={final_gas_limit}");

            // 更新gas_limit
            gas_limit = final_gas_limit;

            // 发送计算完成状态
            let _ = app_handle.emit("transfer_status_update", serde_json::json!({
                "index": index - 1,
                "error_msg": "计算完成",
                "exec_status": "1"
            }));

            let actual_gas_fee = gas_price * gas_limit;
            let final_remaining = balance - max_transfer_amount - actual_gas_fee;
            println!("序号：{index}, 最终: transfer={max_transfer_eth} ETH, gas_fee={actual_gas_fee}, remaining={final_remaining} WEI");

            max_transfer_amount
        }
        "2" => {
            // 转账固定数量
            let amount = parse_ether_to_wei_f64(config.transfer_amount)?;
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "3" => {
            // 转账随机数量
            let mut rng = rand::thread_rng();
            let random_amount = rng.gen_range(config.transfer_amount_list[0]..=config.transfer_amount_list[1]);
            // 根据精度设置格式化随机金额
            let formatted_amount = format!("{:.precision$}", random_amount, precision = config.amount_precision as usize);
            let precise_amount: f64 = formatted_amount.parse()?;
            let amount = parse_ether_to_wei_f64(precise_amount)?;
            if amount >= balance {
                return Err("当前余额不足，不做转账操作！".into());
            }
            amount
        }
        "4" => {
            // 剩余随机数量
            // 使用format_wei_to_ether替代format_ether，确保精度不丢失
            let balance_ether_str = format_wei_to_ether(balance);
            let balance_f64: f64 = balance_ether_str.parse::<f64>().map_err(|e| {
                format!("余额转换失败: {e}, 原始余额: {balance}, format_wei_to_ether结果: {balance_ether_str}")
            })?;
            

            
            // 使用之前已获取的gas_limit
            
            let gas_fee = gas_price * gas_limit;
            let gas_fee_ether_str = format_wei_to_ether(gas_fee);
            let gas_fee_ether: f64 = gas_fee_ether_str.parse::<f64>().map_err(|e| {
                format!("gas费用转换失败: {e}, gas_fee: {gas_fee}, format_wei_to_ether结果: {gas_fee_ether_str}")
            })?;
            

            
            // 可用于转账的余额 = 总余额 - Gas费用
            let available_balance = balance_f64 - gas_fee_ether;
            
            println!("序号：{index}, 总余额: {balance_f64}, Gas费用: {gas_fee_ether}, 可用余额: {available_balance}");
            
            // 检查可用余额是否足够满足最小剩余数量要求
            if available_balance <= config.left_amount_list[1] {
                return Err(format!(
                    "当前可用余额为：{} (总余额: {} - Gas费用: {})，无法满足最大剩余数量 {} 要求，不做转账操作！",
                    available_balance, balance_f64, gas_fee_ether, config.left_amount_list[1]
                ).into());
            }
            
            let mut rng = rand::thread_rng();
            let left_amount = rng.gen_range(config.left_amount_list[0]..=config.left_amount_list[1]);
            let transfer_amount_f64 = available_balance - left_amount;
            
            if transfer_amount_f64 <= 0.0 {
                return Err(format!(
                    "计算转账金额为负数或零：可用余额 {available_balance} - 剩余数量 {left_amount} = {transfer_amount_f64}，不做转账操作！"
                ).into());
            }
            
            // 根据精度设置格式化转账金额
            let formatted_amount = format!("{:.precision$}", transfer_amount_f64, precision = config.amount_precision as usize);
            let precise_amount: f64 = formatted_amount.parse()?;
            
            println!("序号：{index}, 剩余数量: {left_amount}, 转账金额: {transfer_amount_f64} (格式化后: {precise_amount})");
            
            parse_ether_to_wei_f64(precise_amount)?
        }
        _ => return Err("无效的转账类型".into()),
    };
    
    println!("序号：{}, 转账数量为: {}", index, format_wei_to_ether(transfer_amount));
    
    // 构建交易（使用之前已获取的gas_limit）
    let tx = TransactionRequest {
        from: Some(wallet_address),
        to: Some(to_address.into()),
        value: Some(transfer_amount),
        gas_price: Some(gas_price.to::<u128>()),
        gas: Some(gas_limit.to::<u64>()),
        ..Default::default()
    };
    
    // 再次检查停止状态 - 在发送交易之前 (最关键的拦截点)
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    // 发送交易
    item.error_msg = "发送交易...".to_string();
    // 发送状态更新事件到前端
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": item.error_msg.clone(),
        "exec_status": "1"
    }));
    
let signer_provider = create_signer_provider(&config.chain, config.window_id.as_deref(), &wallet).await?;
    let pending_tx = signer_provider.send_transaction(tx).await.map_err(|e| {
        format!("发送交易失败 (RPC: {rpc_url}): {e}")
    })?;
    
    let tx_hash = *pending_tx.tx_hash();
    println!("序号：{index}, 交易 hash 为：{tx_hash:?}");
    
    // 等待交易确认（设置30秒超时）
    item.error_msg = "等待交易结果...".to_string();
    // 发送状态更新事件到前端
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": item.error_msg.clone(),
        "exec_status": "1"
    }));
    
    println!("[DEBUG] 开始等待交易确认，设置30秒超时...");
    let receipt = match tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        pending_tx.get_receipt()
    ).await {
        Ok(result) => {
            result.map_err(|e| {
                format!("等待交易确认失败 (RPC: {rpc_url}): {e}")
            })?
        }
        Err(_) => {
            let timeout_msg = format!("等待交易确认超时 (RPC: {rpc_url}) - 超过30秒未收到确认，交易哈希: {tx_hash:?}");
            println!("[ERROR] {timeout_msg}");
            return Err(timeout_msg.into());
        }
    };
    
    if receipt.status() {
        Ok(format!("{:?}", receipt.transaction_hash))
    } else {
        Err(format!("交易失败 (RPC: {rpc_url})").into())
    }
}

// Tauri命令：查询余额
#[tauri::command]
pub async fn query_balance(
    chain: String,
    address: String,
) -> Result<String, String> {
    match query_balance_internal(chain, address).await {
        Ok(balance) => Ok(balance),
        Err(e) => Err(e.to_string()),
    }
}

// 内部查询余额实现
async fn query_balance_internal(
    chain: String,
    address: String,
) -> Result<String, Box<dyn std::error::Error>> {
    let provider = create_provider(&chain, None).await?;
    if address.trim().is_empty() {
        return Err("查询地址不能为空！".into());
    }
    let address: Address = address.parse().map_err(|e| {
        format!("地址格式错误: {e}，请检查地址格式是否正确")
    })?;
    let balance = match provider.get_balance(address).await {
        Ok(balance) => balance,
        Err(e) => {
             // 获取当前使用的RPC URL
             let error_msg = e.to_string();
             let rpc_url = if let Some(rpc_config) = get_rpc_config(&chain).await {
                 match rpc_config.get_random_rpc() {
                     Ok(url) => url.to_string(),
                     Err(e) => format!("获取RPC地址失败: {e}")
                 }
             } else {
                 "未知RPC".to_string()
             };
             return Err(format!("获取余额失败 (RPC: {rpc_url}): {error_msg}").into());
         }
    };
    let balance_str = format_wei_to_ether(balance);
    Ok(balance_str)
}

// ========== 狂暴模式相关结构和方法 ==========

// 快速转账结果（只返回交易哈希，不等待确认）
#[derive(Debug, Serialize, Deserialize)]
pub struct FastTransferResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub error: Option<String>,
}


// 交易状态检查结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionStatusResult {
    pub confirmed: bool,
    pub success: Option<bool>,  // None表示还在pending，Some(true)表示成功，Some(false)表示失败
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchTransactionStatusResult {
    pub hash: String,
    pub status: TransactionStatusResult,
}

// Tauri命令：批量检查交易状态
#[tauri::command]
pub async fn check_transactions_status_batch(
    chain: String,
    tx_hashes: Vec<String>,
) -> Result<Vec<BatchTransactionStatusResult>, String> {
    match check_transactions_status_batch_internal(chain, tx_hashes).await {
        Ok(results) => Ok(results),
        Err(e) => Err(e.to_string()),
    }
}

// 内部批量检查交易状态实现
async fn check_transactions_status_batch_internal(
    chain: String,
    tx_hashes: Vec<String>,
) -> Result<Vec<BatchTransactionStatusResult>, Box<dyn std::error::Error>> {
    if tx_hashes.is_empty() {
        return Ok(Vec::new());
    }

    // 复用同一个Provider
    let provider = create_provider(&chain, None).await?;
    
    let mut results = Vec::new();
    
    // 并发检查还是顺序检查？
    // 为了避免对RPC造成瞬间过大压力，这里使用顺序检查，因为Provider复用已经减少了很大开销
    // 如果需要进一步优化，可以使用FuturesUnordered进行并发，但要注意RPC限制
    
    for tx_hash in tx_hashes {
        // 解析交易哈希
        let hash_res: Result<alloy::primitives::B256, _> = tx_hash.parse();
        
        match hash_res {
            Ok(hash) => {
                // 获取交易回执
                match provider.get_transaction_receipt(hash).await {
                    Ok(Some(receipt)) => {
                        let success = receipt.status();
                        results.push(BatchTransactionStatusResult {
                            hash: tx_hash,
                            status: TransactionStatusResult {
                                confirmed: true,
                                success: Some(success),
                                error: if success { None } else { Some("交易执行失败".to_string()) },
                            }
                        });
                    }
                    Ok(None) => {
                        results.push(BatchTransactionStatusResult {
                            hash: tx_hash,
                            status: TransactionStatusResult {
                                confirmed: false,
                                success: None,
                                error: None,
                            }
                        });
                    }
                    Err(e) => {
                        // 单个失败不影响整体，标记为查询失败（或者视为pending）
                        // 这里我们返回confirmed=false但带有错误信息，或者直接忽略
                        // 为了前端好处理，我们返回一个带有错误信息的pending状态
                         results.push(BatchTransactionStatusResult {
                            hash: tx_hash,
                            status: TransactionStatusResult {
                                confirmed: false,
                                success: None,
                                error: Some(format!("查询失败: {e}")),
                            }
                        });
                    }
                }
            }
            Err(e) => {
                results.push(BatchTransactionStatusResult {
                    hash: tx_hash,
                    status: TransactionStatusResult {
                        confirmed: false,
                        success: Some(false),
                        error: Some(format!("哈希格式错误: {e}")),
                    }
                });
            }
        }
    }
    
    Ok(results)
}

// Tauri命令：快速基础币转账（狂暴模式 - 只提交不等待确认）
#[tauri::command]
pub async fn base_coin_transfer_fast<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    item: TransferItem,
    config: TransferConfig,
) -> Result<FastTransferResult, String> {
    match base_coin_transfer_fast_internal(app_handle, index, item, config).await {
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

// 内部快速基础币转账实现（只提交交易，不等待确认）
async fn base_coin_transfer_fast_internal<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    index: usize,
    mut item: TransferItem,
    config: TransferConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    item.retry_flag = false;
    
    // 0. 获取窗口ID并检查停止状态
    let window_id = config.window_id.as_deref().unwrap_or("");
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    // 创建Provider (复用)
    // 这是一个优化：只创建一个Provider用于后续的所有查询，减少RPC连接开销和延迟
    let provider = create_provider(&config.chain, config.window_id.as_deref()).await
        .map_err(|e| format!("获取RPC提供商失败: {e}"))?;
    
    // 创建钱包
    let wallet = item.private_key.use_secret(|pk| {
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
    
    // 获取链ID
    let chain_id = match ProviderUtils::get_chain_id(&config.chain).await {
        Ok(id) => id,
        Err(_) => {
            match get_rpc_config(&config.chain).await {
                Some(c) => c.chain_id,
                None => {
                    return Err(format!(
                        "无法获取链 '{}' 的配置信息",
                        config.chain
                    ).into());
                }
            }
        }
    };
    let wallet = wallet.with_chain_id(Some(chain_id));
    let wallet_address = wallet.address();
    
    // 解析目标地址
    if item.to_addr.trim().is_empty() {
        return Err("目标地址不能为空！".into());
    }
    let to_address: Address = item.to_addr.parse().map_err(|e| {
        format!("目标地址格式错误: {e}")
    })?;
    
    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    // 获取余额 (复用Provider)
    let balance = provider.get_balance(wallet_address).await.map_err(|e| {
        format!("获取余额失败: {e}")
    })?;
    
    if balance.is_zero() {
        return Err("当前余额为0，无法进行转账操作！".into());
    }
    
    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    // 获取Gas Price (复用Provider)
    let gas_price = TransferUtils::get_gas_price(&config, provider.clone()).await?;
    
    if gas_price.is_zero() {
        return Err("获取到的 gas price 为0".into());
    }
    
    // 检查停止状态
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    // 获取Gas Limit (复用Provider)
    let gas_limit = match config.limit_type.as_str() {
        "1" => {
            let estimate_amount = parse_ether_to_wei_f64(0.000003)?;
            // 复用Provider
            TransferUtils::get_gas_limit(
                &config,
                provider.clone(),
                wallet_address,
                to_address,
                estimate_amount,
            ).await.map_err(|e| format!("获取gas limit失败: {e}"))?
        }
        "2" => U256::from(config.limit_count),
        "3" => {
            let mut rng = rand::thread_rng();
            let random_limit = rng.gen_range(config.limit_count_list[0]..=config.limit_count_list[1]);
            U256::from(random_limit)
        }
        _ => U256::from(21000),
    };
    
    // 计算转账金额（简化版本，不做复杂的二分法优化以提高速度）
    let transfer_amount = match config.transfer_type.as_str() {
        "1" => {
            // 全部转账 - 快速计算
            let gas_fee = gas_price * gas_limit;
            let safety_margin = gas_fee * U256::from(10) / U256::from(100); // 10%安全边际
            if balance <= gas_fee + safety_margin {
                return Err("余额不足支付Gas费用".into());
            }
            balance - gas_fee - safety_margin
        }
        "2" => {
            let amount = parse_ether_to_wei_f64(config.transfer_amount)?;
            if amount >= balance {
                return Err("余额不足".into());
            }
            amount
        }
        "3" => {
            let mut rng = rand::thread_rng();
            let random_amount = rng.gen_range(config.transfer_amount_list[0]..=config.transfer_amount_list[1]);
            let formatted_amount = format!("{:.precision$}", random_amount, precision = config.amount_precision as usize);
            let precise_amount: f64 = formatted_amount.parse()?;
            let amount = parse_ether_to_wei_f64(precise_amount)?;
            if amount >= balance {
                return Err("余额不足".into());
            }
            amount
        }
        "4" => {
            let balance_f64: f64 = format_wei_to_ether(balance).parse()?;
            let gas_fee = gas_price * gas_limit;
            let gas_fee_ether: f64 = format_wei_to_ether(gas_fee).parse()?;
            let available_balance = balance_f64 - gas_fee_ether;
            
            if available_balance <= config.left_amount_list[1] {
                return Err("余额不足".into());
            }
            
            let mut rng = rand::thread_rng();
            let left_amount = rng.gen_range(config.left_amount_list[0]..=config.left_amount_list[1]);
            let transfer_amount_f64 = available_balance - left_amount;
            
            if transfer_amount_f64 <= 0.0 {
                return Err("计算转账金额为负数或零".into());
            }
            
            let formatted_amount = format!("{:.precision$}", transfer_amount_f64, precision = config.amount_precision as usize);
            let precise_amount: f64 = formatted_amount.parse()?;
            parse_ether_to_wei_f64(precise_amount)?
        }
        _ => return Err("无效的转账类型".into()),
    };
    
    // 发送状态更新
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": "发送交易中...",
        "exec_status": "1"
    }));
    
    // 构建并发送交易
    let tx = TransactionRequest {
        from: Some(wallet_address),
        to: Some(to_address.into()),
        value: Some(transfer_amount),
        gas_price: Some(gas_price.to::<u128>()),
        gas: Some(gas_limit.to::<u64>()),
        ..Default::default()
    };
    
    // 再次检查停止状态 - 在发送交易之前 (最关键的拦截点)
    if !window_id.is_empty() && get_stop_flag(window_id) {
        return Err("用户已停止转账任务".into());
    }

    let signer_provider = create_signer_provider(&config.chain, config.window_id.as_deref(), &wallet).await?;
    let pending_tx = signer_provider.send_transaction(tx).await.map_err(|e| {
        format!("发送交易失败: {e}")
    })?;
    
    let tx_hash = pending_tx.tx_hash();
    let tx_hash_str = format!("{tx_hash:?}");
    
    println!("[狂暴模式] 序号：{index}, 交易已提交，hash: {tx_hash_str}");
    
    // 发送状态更新 - 交易已提交，等待确认
    let _ = app_handle.emit("transfer_status_update", serde_json::json!({
        "index": index - 1,
        "error_msg": format!("已提交，等待确认: {}", &tx_hash_str[..20]),
        "exec_status": "1"
    }));
    
    // 不等待确认，直接返回交易哈希
    Ok(tx_hash_str)
}

// Tauri命令：检查交易状态
#[tauri::command]
pub async fn check_transaction_status(
    chain: String,
    tx_hash: String,
) -> Result<TransactionStatusResult, String> {
    match check_transaction_status_internal(chain, tx_hash).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

// 内部检查交易状态实现
pub(crate) async fn check_transaction_status_internal(
    chain: String,
    tx_hash: String,
) -> Result<TransactionStatusResult, Box<dyn std::error::Error>> {
    let provider = create_provider(&chain, None).await?;
    
    // 解析交易哈希
    let hash: alloy::primitives::B256 = tx_hash.parse().map_err(|e| {
        format!("交易哈希格式错误: {e}")
    })?;
    
    // 获取交易回执
    match provider.get_transaction_receipt(hash).await {
        Ok(Some(receipt)) => {
            // 交易已确认
            let success = receipt.status();
            Ok(TransactionStatusResult {
                confirmed: true,
                success: Some(success),
                error: if success { None } else { Some("交易执行失败".to_string()) },
            })
        }
        Ok(None) => {
            // 交易还在pending
            Ok(TransactionStatusResult {
                confirmed: false,
                success: None,
                error: None,
            })
        }
        Err(e) => {
            Err(format!("查询交易状态失败: {e}").into())
        }
    }
}

// ========== 原有代码 ==========

// 检查钱包最近转账记录的结果结构
#[derive(Debug, Serialize, Deserialize)]
pub struct RecentTransferResult {
    pub has_recent_transfer: bool,
    pub transaction_count: u32,
    pub latest_transaction_hash: Option<String>,
}

// Tauri命令：检查钱包最近转账记录
#[tauri::command]
pub async fn check_wallet_recent_transfers(
    chain: String,
    private_key: SecureMemory,
    target_address: String,
    start_timestamp: u64,
    coin_type: String,
    contract_address: Option<String>,
    amount: Option<String>,
) -> Result<RecentTransferResult, String> {
    match check_wallet_recent_transfers_internal(
        chain,
        private_key,
        target_address,
        start_timestamp,
        coin_type,
        contract_address,
        amount,
    ).await {
        Ok(result) => Ok(result),
        Err(e) => Err(e.to_string()),
    }
}

// 内部检查钱包最近转账记录实现
async fn check_wallet_recent_transfers_internal(
    chain: String,
    private_key: SecureMemory,
    target_address: String,
    start_timestamp: u64,
    coin_type: String,
    contract_address: Option<String>,
    amount: Option<String>,
) -> Result<RecentTransferResult, Box<dyn std::error::Error>> {
    // 创建Provider
    let provider = create_provider(&chain, None).await?;

    // 使用 SecureMemory 安全获取私钥并创建钱包
    let wallet = private_key.use_secret(|key_str| {
        // 处理私钥格式
        let private_key = if key_str.starts_with("0x") || key_str.starts_with("0X") {
            &key_str[2..]
        } else {
            key_str
        };
        private_key.parse::<PrivateKeySigner>()
    }).map_err(|e| format!("解密私钥失败: {e}"))??;
    let wallet_address = wallet.address();
    
    // 解析目标地址
    let target_addr: Address = target_address.parse().map_err(|e| {
        format!("目标地址格式错误: {e}")
    })?;
    
    // 获取当前区块号
    let current_block = match provider.get_block_number().await {
        Ok(block) => block,
        Err(e) => {
             // 获取当前使用的RPC URL
             let error_msg = e.to_string();
             let rpc_url = if let Some(rpc_config) = get_rpc_config(&chain).await {
                 match rpc_config.get_random_rpc() {
                     Ok(url) => url.to_string(),
                     Err(e) => format!("获取RPC地址失败: {e}")
                 }
             } else {
                 "未知RPC".to_string()
             };
             return Err(format!("获取当前区块号失败 (RPC: {rpc_url}): {error_msg}").into());
         }
    };
    
    // 计算开始查询的区块号（简化实现：从最近1000个区块开始查询）
    let current_block_u64 = current_block;
    let start_block = current_block_u64.saturating_sub(1000);
    
    // 解析预期的转账金额（如果有）
    let expected_wei_value = if let Some(amt_str) = &amount {
        if !amt_str.is_empty() {
            if let Ok(val) = amt_str.parse::<f64>() {
                // 仅当币种为base时，我们确定金额单位转换关系（Ether -> Wei）
                // 对于Token，由于不知道Decimals，暂不进行金额严格校验
                if coin_type == "base" {
                    parse_ether_to_wei_f64(val).ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut transaction_count = 0u32;
    let mut latest_transaction_hash: Option<String> = None;
    let mut has_recent_transfer = false;
    
    // 查询指定区块范围内的交易
    for block_num in start_block..=current_block_u64 {
        if let Ok(Some(block)) = provider.get_block_by_number(block_num.into()).full().await {
            // 检查区块时间戳是否在指定时间之后
            if block.header.timestamp < start_timestamp {
                continue;
            }
            
            // 遍历区块中的所有交易
            let transactions = match block.transactions {
                alloy_rpc_types_eth::BlockTransactions::Full(txs) => txs,
                _ => Vec::new(),
            };

            for tx in transactions {
                // 检查交易是否来自指定钱包地址
                let from_address = tx.inner.signer();
                if from_address == wallet_address {
                    transaction_count += 1;
                    
                    // 根据币种类型检查交易
                    match coin_type.as_str() {
                        "base" => {
                            // 基础币转账：检查to地址
                            if let Some(to_addr) = tx.inner.to() {
                                if to_addr == target_addr {
                                    // 如果设置了金额校验，则需检查金额是否一致
                                    if let Some(expected_wei) = expected_wei_value {
                                        // 允许极小的误差（浮点数转换可能导致1wei的差异），或者严格相等
                                        // 这里使用严格相等，因为parse_ether_to_wei_f64应该是确定性的
                                        if tx.inner.value() != expected_wei {
                                            continue;
                                        }
                                    }
                                    
                                    has_recent_transfer = true;
                                    latest_transaction_hash = Some(format!("{:?}", tx.inner.hash()));
                                }
                            }
                        }
                        "token" => {
                            // 代币转账：检查合约调用和事件日志
                            if let Some(contract_addr) = &contract_address {
                                let contract_address: Address = contract_addr.parse().map_err(|e| {
                                    format!("合约地址格式错误: {e}")
                                })?;
                                
                                // 检查是否是对指定合约的调用
                                if let Some(to_addr) = tx.inner.to() {
                                    if to_addr == contract_address {
                                        // 获取交易回执以检查事件日志
                                        if let Ok(Some(receipt)) = provider.get_transaction_receipt(*tx.inner.hash()).await {
                                            // 检查Transfer事件日志
                                            for log in receipt.inner.logs() {
                                                // Transfer事件的topic0是keccak256("Transfer(address,address,uint256)")
                                                let transfer_topic = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
                                                
                                                if !log.topics().is_empty() && 
                                                   format!("{:?}", log.topics()[0]) == transfer_topic &&
                                                   log.topics().len() >= 3 {
                                                    // topics[1]是from地址，topics[2]是to地址
                                                    let to_topic = log.topics()[2];
                                                    // 将topic转换为地址（去掉前12个字节的0）
                                                    let to_bytes = &to_topic.as_slice()[12..];
                                                    let to_address = Address::from_slice(to_bytes);
                                                    
                                                    if to_address == target_addr {
                                                        has_recent_transfer = true;
                                                        latest_transaction_hash = Some(format!("{:?}", tx.inner.hash()));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            // 未知币种类型，跳过
                            continue;
                        }
                    }
                }
            }
        }
    }
    
    Ok(RecentTransferResult {
        has_recent_transfer,
        transaction_count,
        latest_transaction_hash,
    })
}

// ─── Batch Transfer ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct BatchTransferItemFront {
    pub key: String,
    pub private_key: SecureMemory,
    pub address: String,
    pub to_addr: String,
    pub amount: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchTransferParams {
    pub items: Vec<BatchTransferItemFront>,
    pub config: TransferConfig,
    pub thread_count: usize,
    pub fury_mode: bool,
    pub transfer_id: String,
}

#[derive(Debug, Serialize)]
pub struct BatchTransferSummary {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub stopped: bool,
}

#[tauri::command]
pub async fn batch_base_coin_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    params: BatchTransferParams,
) -> Result<BatchTransferSummary, String> {
    run_batch_transfer(app_handle, params).await
}

async fn run_batch_transfer<R: tauri::Runtime>(
    app_handle: tauri::AppHandle<R>,
    params: BatchTransferParams,
) -> Result<BatchTransferSummary, String> {
    let window_id = params.config.window_id.clone().unwrap_or_default();
    reset_stop_flag(&window_id);

    let fury_mode = params.fury_mode || params.thread_count > 90;
    let thread_count = params.thread_count.clamp(1, 200);
    let total_items = params.items.len();

    // 按钱包地址分组，保证同一钱包的 nonce 顺序正确
    let mut wallet_groups: Vec<Vec<BatchTransferItemFront>> = Vec::new();
    {
        let mut addr_to_group: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for item in params.items {
            let addr = item.address.to_lowercase();
            let idx = *addr_to_group.entry(addr).or_insert_with(|| {
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

                // Per-item config: override transfer_amount when item provides own amount
                let mut item_cfg = (*cfg).clone();
                if let Some(ref amt) = front_item.amount {
                    if !amt.is_empty() {
                        if let Ok(v) = amt.parse::<f64>() {
                            item_cfg.transfer_amount = v;
                        }
                    }
                }

                let transfer_item = TransferItem {
                    private_key: front_item.private_key.clone(),
                    to_addr: front_item.to_addr.clone(),
                    error_msg: String::new(),
                    retry_flag: false,
                };

                // Notify frontend: started
                let _ = ah.emit("transfer_item_update", serde_json::json!({
                    "key": front_item.key,
                    "exec_status": "1",
                    "tx_hash": null,
                    "error_msg": if fury_mode { "正在提交..." } else { "执行中..." },
                    "window_id": wid,
                    "transfer_id": *tid,
                }));

                if fury_mode {
                    // Fast submit, then poll for confirmation
                    let fast_result: Result<String, String> = base_coin_transfer_fast_internal(ah.clone(), 0, transfer_item, item_cfg.clone()).await.map_err(|e| e.to_string());
                    match fast_result {
                        Ok(tx_hash) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key,
                                "exec_status": "4",
                                "tx_hash": tx_hash,
                                "error_msg": "已提交，等待上链确认...",
                                "window_id": wid,
                                "transfer_id": *tid,
                            }));

                            let chain = item_cfg.chain.clone();
                            let start = std::time::Instant::now();
                            let max_wait = std::time::Duration::from_secs(90);
                            let poll_interval = std::time::Duration::from_secs(2);
                            let tx_hash_copy = tx_hash.clone();

                            loop {
                                if start.elapsed() > max_wait {
                                    let _ = ah.emit("transfer_item_update", serde_json::json!({
                                        "key": front_item.key,
                                        "exec_status": "3",
                                        "tx_hash": tx_hash_copy,
                                        "error_msg": "上链确认超时",
                                        "window_id": wid,
                                        "transfer_id": *tid,
                                    }));
                                    fc.fetch_add(1, AO::Relaxed);
                                    break;
                                }
                                if get_stop_flag(&wid) {
                                    fc.fetch_add(1, AO::Relaxed);
                                    break;
                                }
                                match check_transaction_status_internal(chain.clone(), tx_hash_copy.clone()).await.map_err(|e| e.to_string()) {
                                    Ok(s) if s.confirmed => {
                                        if s.success == Some(true) {
                                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                                "key": front_item.key,
                                                "exec_status": "2",
                                                "tx_hash": tx_hash_copy,
                                                "error_msg": tx_hash_copy,
                                                "window_id": wid,
                                                "transfer_id": *tid,
                                            }));
                                            sc.fetch_add(1, AO::Relaxed);
                                        } else {
                                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                                "key": front_item.key,
                                                "exec_status": "3",
                                                "tx_hash": tx_hash_copy,
                                                "error_msg": "链上执行失败",
                                                "window_id": wid,
                                                "transfer_id": *tid,
                                            }));
                                            fc.fetch_add(1, AO::Relaxed);
                                        }
                                        break;
                                    }
                                    _ => { sleep(poll_interval).await; }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key,
                                "exec_status": "3",
                                "tx_hash": null,
                                "error_msg": e.to_string(),
                                "window_id": wid,
                                "transfer_id": *tid,
                            }));
                            fc.fetch_add(1, AO::Relaxed);
                        }
                    }
                } else {
                    // Normal mode: wait for on-chain confirmation
                    match base_coin_transfer_internal(ah.clone(), 0, transfer_item, item_cfg.clone()).await {
                        Ok(tx_hash) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key,
                                "exec_status": "2",
                                "tx_hash": tx_hash,
                                "error_msg": tx_hash,
                                "window_id": wid,
                                "transfer_id": *tid,
                            }));
                            sc.fetch_add(1, AO::Relaxed);
                        }
                        Err(e) => {
                            let _ = ah.emit("transfer_item_update", serde_json::json!({
                                "key": front_item.key,
                                "exec_status": "3",
                                "tx_hash": null,
                                "error_msg": e.to_string(),
                                "window_id": wid,
                                "transfer_id": *tid,
                            }));
                            fc.fetch_add(1, AO::Relaxed);
                        }
                    }

                    // 同一钱包的相邻转账之间加延迟（最后一笔不等待）
                    if i < group_len - 1 {
                        let [min_s, max_s] = item_cfg.delay;
                        if min_s > 0 || max_s > 0 {
                            let secs = if max_s > min_s {
                                rand::thread_rng().gen_range(min_s..=max_s)
                            } else {
                                min_s
                            };
                            if secs > 0 { sleep(Duration::from_secs(secs)).await; }
                        }
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
