import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Notification } from '@arco-design/web-vue';

export function useTransfer(options = {}) {
  const {
    data,
    form,
    chainValue,
    currentCoin,
    threadCount,
    enableMultiThread,
    transferConfig,
    showProgress,
    startLoading,
    stopFlag,
    stopStatus,
    transferStartTime,
    transferSessionCompleted,
    updateTransferProgress,
    checkGasPriceForTransfer,
    startGasPriceMonitoring,
    transferPaused,
    pausedTransferData,
    executeTransfer,
  } = options;

  async function ensureBalance(item) {
    if (form.send_type !== '1' && form.send_type !== '4') return;

    const isBase = currentCoin.value.coin_type === 'base';
    const address = item.address;
    if (!address) return;

    try {
      if (isBase) {
        const bal = await invoke('query_balance', { chain: chainValue.value, address });
        item.plat_balance = bal;
        const realIndex = data.value.findIndex(d => d.key === item.key);
        if (realIndex !== -1) {
          data.value[realIndex].plat_balance = bal;
        }
      } else {
        const result = await invoke('query_balances_simple', {
          params: {
            chain: chainValue.value,
            coin_config: {
              coin_type: currentCoin.value.coin_type,
              contract_address: currentCoin.value.contract_address || null,
              abi: currentCoin.value.abi || null,
            },
            items: [{
              key: item.key,
              address,
              private_key: null,
              plat_balance: null,
              coin_balance: null,
              nonce: null,
              exec_status: '0',
              error_msg: null,
              retry_flag: false,
            }],
            only_coin_config: true,
            thread_count: 1,
          },
        });
        if (result?.success && result.items?.length > 0) {
          const resItem = result.items[0];
          item.coin_balance = resItem.coin_balance;
          item.plat_balance = resItem.plat_balance;
          const realIndex = data.value.findIndex(d => d.key === item.key);
          if (realIndex !== -1) {
            data.value[realIndex].coin_balance = resItem.coin_balance;
            data.value[realIndex].plat_balance = resItem.plat_balance;
          }
        }
      }
    } catch (e) {
      console.error('自动查询余额失败', e);
    }
  }

  const retryInProgress = ref(false);
  const retryResults = ref([]);
  const MAX_RETRY_ROUNDS = 3;
  const currentRetryRound = ref(0);
  const lastTransferStartTime = ref(0);

  async function performIntelligentRetry(failedData) {
    if (!transferStartTime.value) {
      console.warn('未找到转账开始时间，使用传统重试方式');
      executeTransfer(failedData);
      return;
    }

    // 检查是否为新一轮任务（根据开始时间变化判断）
    if (transferStartTime.value !== lastTransferStartTime.value) {
      currentRetryRound.value = 0;
      lastTransferStartTime.value = transferStartTime.value;
    }

    if (currentRetryRound.value >= MAX_RETRY_ROUNDS) {
      Notification.warning({
        content: `已达到最大重试次数 (${MAX_RETRY_ROUNDS}次)，停止重试以防止死循环`,
        position: 'topLeft',
      });
      stopStatus.value = true;
      transferSessionCompleted.value = true;
      return;
    }

    currentRetryRound.value++;
    retryInProgress.value = true;
    retryResults.value = [];

    Notification.info({
      content: `开始第 ${currentRetryRound.value}/${MAX_RETRY_ROUNDS} 轮智能重试检查，共 ${failedData.length} 笔失败交易`,
      position: 'topLeft',
    });

    try {
      const retryList = [];
      const concurrency = 5; // 并发检查数

      for (let i = 0; i < failedData.length; i += concurrency) {
        if (stopFlag.value) break;
        const chunk = failedData.slice(i, i + concurrency);
        await Promise.all(
          chunk.map(async (item) => {
            try {
              // 确定用于校验的金额
              // 只有当转账类型为"固定数量"(type='2')时，才能进行精确金额校验
              // 其他类型(全部/随机等)金额不固定，只校验地址
              let amountToCheck = null;
              if (transferConfig.value && transferConfig.value.transfer_type === '2') {
                if (form.amount_from === '1') {
                  amountToCheck = item.amount ? String(item.amount) : null;
                } else {
                  amountToCheck = form.send_count ? String(form.send_count) : null;
                }
              }

              const hasRecentTransfer = await checkRecentTransfer(
                item.private_key,
                item.to_addr,
                transferStartTime.value,
                amountToCheck
              );

              if (hasRecentTransfer) {
                const realIndex = data.value.findIndex(
                  (dataItem) => dataItem.key === item.key
                );
                if (realIndex !== -1) {
                  data.value[realIndex].error_msg =
                    '检测到链上已有相关交易，跳过重试';
                  data.value[realIndex].exec_status = '2';
                  data.value[realIndex].retry_flag = false;
                }
                retryResults.value.push({
                  key: item.key,
                  address: item.to_addr,
                  action: '跳过重试',
                  reason: '检测到链上已有相关交易',
                });
              } else {
                retryList.push(item);
                retryResults.value.push({
                  key: item.key,
                  address: item.to_addr,
                  action: '加入重试',
                  reason: '未检测到相关链上交易',
                });
              }
            } catch (error) {
              console.error(`检查交易失败 ${item.to_addr}:`, error);
              retryList.push(item);
              retryResults.value.push({
                key: item.key,
                address: item.to_addr,
                action: '加入重试',
                reason: '检查失败，保守重试',
              });
            }
          })
        );
      }

      retryInProgress.value = false;

      if (stopFlag.value) {
        Notification.warning({ content: '重试检查已停止', position: 'topLeft' });
        stopStatus.value = true;
        return;
      }

      if (retryList.length > 0) {
        Notification.info({
          content: `智能检查完成，将重试 ${retryList.length} 笔交易`,
          position: 'topLeft',
        });
        
        // 重置待重试数据的状态
        retryList.forEach((item) => {
          const realIndex = data.value.findIndex((d) => d.key === item.key);
          if (realIndex !== -1) {
            data.value[realIndex].exec_status = '0';
            data.value[realIndex].error_msg = `等待重试 (${currentRetryRound.value}/${MAX_RETRY_ROUNDS})...`;
          }
        });

        // 传入 false 以避免重置 transferStartTime
        executeTransfer(retryList, false);
      } else {
        Notification.success({
          content:
            '智能重试检查完成，所有失败交易均已确认成功或跳过',
          position: 'topLeft',
        });
        stopStatus.value = true;
        transferSessionCompleted.value = true;
      }
    } catch (error) {
      console.error('智能重试检查失败:', error);
      retryInProgress.value = false;
      Notification.error({
        content: '智能重试检查异常，停止重试',
        position: 'topLeft',
      });
      stopStatus.value = true;
      transferSessionCompleted.value = true;
    }
  }

  async function checkRecentTransfer(privateKey, targetAddress, startTime, amount = null) {
    try {
      const result = await invoke('check_wallet_recent_transfers', {
        chain: chainValue.value,
        private_key: privateKey,
        target_address: targetAddress.toLowerCase(),
        start_timestamp: startTime,
        coin_type: currentCoin.value.coin_type,
        contract_address:
            currentCoin.value.coin_type === 'token'
                ? currentCoin.value.contract_address
                : null,
        amount: amount,
      });

      return result.has_recent_transfer || false;
    } catch (error) {
      console.error('查询链上交易失败:', error);
      throw error;
    }
  }

  async function transferFnc(inputData) {
    try {
      if (transferConfig.value && transferConfig.value.window_id) {
        await invoke('reset_transfer_stop', { windowId: transferConfig.value.window_id });
      }
    } catch (e) {
      console.error('重置停止标志失败:', e);
    }

    await iterTransfer(inputData)
        .then(async () => {
          if (stopFlag.value) {
            Notification.warning({
              content: '已停止执行！',
              position: 'topLeft',
            });
          } else {
            const retryData = inputData.filter(
                (item) => item.retry_flag === true,
            );
            if (form.error_retry === '1' && retryData.length > 0) {
              await performIntelligentRetry(retryData);
            } else {
              const successCount = inputData.filter(
                  (item) => item.exec_status === '2',
              ).length;
              const totalCount = inputData.length;

              if (successCount > 0) {
                Notification.success({
                  content: `执行完成！成功转账 ${successCount}/${totalCount} 笔`,
                  position: 'topLeft',
                });
              } else {
                Notification.warning({
                  content: '执行完成，但没有成功的转账',
                  position: 'topLeft',
                });
              }

              stopStatus.value = true;
              transferSessionCompleted.value = true;
            }
          }
          startLoading.value = false;
          stopFlag.value = false;
          showProgress.value = false;
        })
        .catch(() => {
          Notification.error({ content: '执行失败！', position: 'topLeft' });
          startLoading.value = false;
          stopStatus.value = true;
          showProgress.value = false;
        });
  }

  async function iterTransfer(accountData) {
    const isFuryMode = threadCount.value > 90;

    if (isFuryMode) {
      Notification.info({
        content: '狂暴模式已激活：交易将快速批量提交，后端并行确认结果',
        position: 'topLeft',
      });
    }

    // key→realIndex lookup for event dispatch
    const keyToRealIndex = new Map();
    data.value.forEach((d, idx) => keyToRealIndex.set(d.key, idx));

    // Build per-item batch payload (only pending rows)
    const items = accountData
      .filter((item) => item.exec_status === '0')
      .map((item) => {
        const perItemAmount =
          form.amount_from === '1'
            ? item.amount && item.amount.trim() !== '' ? item.amount : null
            : null;
        return {
          key: item.key,
          private_key: item.private_key,
          address: (item.address || '').toLowerCase(),
          to_addr: item.to_addr,
          amount: perItemAmount,
        };
      });

    if (items.length === 0) return;

    const { listen } = await import('@tauri-apps/api/event');
    const transferId = `eth_transfer_${Date.now()}`;

    const unlisten = await listen('transfer_item_update', (event) => {
      const { key, exec_status, error_msg, transfer_id } = event.payload;
      if (transfer_id !== transferId) return;
      const realIdx = keyToRealIndex.get(key);
      if (realIdx === undefined) return;
      data.value[realIdx].exec_status = exec_status;
      data.value[realIdx].error_msg = error_msg ?? '';
      if (exec_status === '3') data.value[realIdx].retry_flag = true;
      if (exec_status === '2') data.value[realIdx].retry_flag = false;
      updateTransferProgress();
    });

    const isToken = currentCoin.value.coin_type === 'token';
    const config = {
      ...transferConfig.value,
      transfer_amount:
        form.send_count && form.send_count.trim() !== ''
          ? Number(form.send_count)
          : transferConfig.value?.transfer_amount ?? 0,
      ...(isToken ? { contract_address: currentCoin.value.contract_address } : {}),
    };

    try {
      await invoke(isToken ? 'batch_token_transfer' : 'batch_base_coin_transfer', {
        params: {
          items,
          config,
          thread_count: threadCount.value,
          fury_mode: isFuryMode,
          transfer_id: transferId,
        },
      });
    } finally {
      unlisten();
    }
  }

  function stopTransfer() {
    startLoading.value = false;
    stopFlag.value = true;
    stopStatus.value = true;
    showProgress.value = false;
    
    if (transferConfig.value && transferConfig.value.window_id) {
      invoke('stop_transfer', { windowId: transferConfig.value.window_id }).catch(e => {
        console.error('发送停止命令失败:', e);
      });
    }
  }

  return {
    transferFnc,
    stopTransfer,
    performIntelligentRetry,
    iterTransfer,
    retryInProgress,
    retryResults,
  };
}
