import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Keypair } from '@solana/web3.js';
import bs58 from 'bs58';
import QRCode from 'qrcode';
import { Notification } from '@arco-design/web-vue';
import { debounce as customDebounce } from '@/utils/debounce.js';

export function useTip(options = {}) {
  const {
    chainValue,
    currentChain,
    currentCoin,
  } = options;

  const showCelebration = ref(false);
  const showTipModal = ref(false);
  const tipAmount = ref('');
  const tipPrivateKey = ref('');
  const tipLoading = ref(false);
  // Solana developer address (base58)
  const developerAddress = ref('7EcDhSYGxXyscszYEp35KHN8vvw3svAuLKTzXwCFLtV');
  const tipAmountOptions = ['0.001', '0.005', '0.01', '0.05', '0.1'];

  const tipMode = ref('qrcode');
  const showQRCode = computed(() => tipMode.value === 'qrcode');
  const showPrivateKeyInput = computed(() => tipMode.value === 'privatekey');

  const qrCodeDataURL = ref('');

  async function generateQRCode() {
    try {
      const dataURL = await QRCode.toDataURL(developerAddress.value, {
        width: 200,
        margin: 2,
        color: {
          dark: '#000000',
          light: '#FFFFFF',
        },
      });
      qrCodeDataURL.value = dataURL;
    } catch (error) {
      console.error('生成二维码失败:', error);
      Notification.error({ content: '生成二维码失败', position: 'topLeft' });
    }
  }

  watch(showTipModal, (newValue) => {
    if (newValue && tipMode.value === 'qrcode') {
      generateQRCode();
    }
  });

  function switchTipMode(mode) {
    tipMode.value = mode;
    if (mode === 'privatekey') {
      tipPrivateKey.value = '';
    }
  }

  function copyAddressToClipboard(address) {
    navigator.clipboard.writeText(address);
    Notification.success({
      title: '已复制',
      content: '地址已复制到剪贴板',
    });
  }

  function triggerCelebration() {
    showCelebration.value = true;

    setTimeout(() => {
      showCelebration.value = false;
      showTipModal.value = true;
    }, 3000);
  }

  function skipTip() {
    showTipModal.value = false;
    tipAmount.value = '';
    tipPrivateKey.value = '';
    Notification.info({ content: '感谢您使用本工具！', position: 'topLeft' });
  }

  const tipWalletBalance = ref({
    valid: false,
    balance: 0,
    address: '',
    loading: false,
    error: null,
    hasAttempted: false,
  });

  function validatePrivateKey(privateKey) {
    try {
      if (!privateKey || typeof privateKey !== 'string') {
        return false;
      }
      const cleanKey = privateKey.trim();
      const secretKey = bs58.decode(cleanKey);
      if (secretKey.length !== 64) {
        return false;
      }
      Keypair.fromSecretKey(secretKey);
      return true;
    } catch (error) {
      return false;
    }
  }

  function deriveAddress(privateKey) {
    const secretKey = bs58.decode(privateKey.trim());
    const keypair = Keypair.fromSecretKey(secretKey);
    return keypair.publicKey.toBase58();
  }

  async function queryTipWalletBalance() {
    if (!tipPrivateKey.value || !tipPrivateKey.value.trim()) {
      tipWalletBalance.value = {
        valid: false,
        balance: 0,
        address: '',
        loading: false,
        error: null,
        hasAttempted: false,
      };
      return;
    }

    if (!validatePrivateKey(tipPrivateKey.value.trim())) {
      tipWalletBalance.value = {
        valid: false,
        balance: 0,
        address: '',
        error: '私钥格式不正确',
        loading: false,
        hasAttempted: true,
      };
      return;
    }

    try {
      const address = deriveAddress(tipPrivateKey.value.trim());

      tipWalletBalance.value = {
        valid: false,
        balance: 0,
        address,
        loading: true,
        error: null,
        hasAttempted: true,
      };

      let balance = 0;

      if (currentCoin.value?.coin_type === 'base') {
        const result = await invoke('query_balance', {
          chain: chainValue.value,
          address: address,
        });

        if (typeof result === 'string') {
          balance = parseFloat(result || '0');
        } else if (typeof result === 'number') {
          balance = result;
        }
      } else if (currentCoin.value?.coin_type === 'token') {
        const params = {
          chain: chainValue.value,
          coin_config: {
            coin_type: currentCoin.value.coin_type,
            contract_address: currentCoin.value.contract_address || null,
            abi: currentCoin.value.abi || null,
          },
          items: [
            {
              key: address,
              address: address,
              private_key: null,
              plat_balance: null,
              coin_balance: null,
              nonce: null,
              exec_status: '0',
              error_msg: null,
              retry_flag: false,
            },
          ],
          only_coin_config: true,
          thread_count: 1,
        };

        const result = await invoke('query_balances_simple', { params });

        if (result && result.success && result.items && result.items.length > 0) {
          const item = result.items[0];
          if (item.exec_status === '2') {
            balance = parseFloat(item.coin_balance || '0');
          } else {
            throw new Error(item.error_msg || '代币余额查询失败');
          }
        } else {
          throw new Error('代币余额查询失败');
        }
      }

      tipWalletBalance.value = {
        valid: true,
        balance,
        address,
        loading: false,
        error: null,
        hasAttempted: true,
        sufficient: tipAmount.value
            ? balance >= parseFloat(tipAmount.value)
            : true,
      };
    } catch (error) {
      console.error('查询打赏钱包余额失败:', error);
      tipWalletBalance.value = {
        valid: false,
        balance: 0,
        address: tipWalletBalance.value.address || '',
        error: '余额查询失败: ' + error.message,
        loading: false,
        hasAttempted: true,
      };
    }
  }

  const debouncedQueryTipWalletBalance = customDebounce(queryTipWalletBalance, 1000);

  watch(tipPrivateKey, debouncedQueryTipWalletBalance);
  watch(currentCoin, queryTipWalletBalance);

  const tipBalanceSufficient = computed(() => {
    if (!tipWalletBalance.value.valid || !tipAmount.value) return true;
    return tipWalletBalance.value.balance >= parseFloat(tipAmount.value);
  });

  const shouldShowTipWalletStatus = computed(() => {
    return tipPrivateKey.value && tipPrivateKey.value.trim().length > 0;
  });

  async function sendTip() {
    if (!tipAmount.value || parseFloat(tipAmount.value) <= 0) {
      Notification.warning({
        content: '请输入有效的打赏金额',
        position: 'topLeft',
      });
      return;
    }

    if (!tipPrivateKey.value || !tipPrivateKey.value.trim()) {
      Notification.warning({ content: '请输入私钥', position: 'topLeft' });
      return;
    }

    if (!validatePrivateKey(tipPrivateKey.value.trim())) {
      Notification.warning({
        content: '私钥格式不正确',
        position: 'topLeft',
      });
      return;
    }

    tipLoading.value = true;

    try {
      const tipData = {
        private_key: tipPrivateKey.value.trim(),
        to_addr: developerAddress.value,
        error_msg: '',
        error_count: 0,
        retry_flag: false,
      };

      const config = {
        error_count_limit: 3,
        error_retry: '0',
        chain: chainValue.value,
        chainLayer: currentChain.value.layer,
        l1: currentChain.value.l1,
        scalar: currentChain.value.scalar,
        delay: [1, 3],
        transfer_type: '2',
        transfer_amount: parseFloat(tipAmount.value),
        transfer_amount_list: [0, 0],
        left_amount_list: [0, 0],
        amount_precision: 6,
        limit_type: '1',
        limit_count: 21000,
        limit_count_list: [21000, 30000],
        gas_price_type: '1',
        gas_price_rate: 0.05,
        gas_price: 30,
        max_gas_price: 0,
      };

      let result;
      const tipTransferIndex = 999999;

      if (currentCoin.value.coin_type === 'base') {
        result = await invoke('base_coin_transfer', {
          index: tipTransferIndex,
          item: tipData,
          config: config,
        });
      } else if (currentCoin.value.coin_type === 'token') {
        result = await invoke('token_transfer', {
          index: tipTransferIndex,
          item: tipData,
          config: {
            ...config,
            contract_address: currentCoin.value.contract_address,
            abi: currentCoin.value.abi,
          },
        });
      }

      if (typeof result === 'object' && result !== null) {
        if (result.success && result.tx_hash) {
          Notification.success({
            title: '打赏成功！',
            content: '感谢您的支持！',
            duration: 5000,
            position: 'topLeft',
          });
        } else {
          throw new Error(result.error || '打赏失败');
        }
      } else {
        Notification.success({
          title: '打赏成功！',
          content: '感谢您的支持！',
          duration: 3000,
          position: 'topLeft',
        });
      }

      showTipModal.value = false;
      tipAmount.value = '';
      tipPrivateKey.value = '';
    } catch (error) {
      console.error('打赏失败:', error);
      Notification.error('打赏失败: ' + error.message);
    } finally {
      tipLoading.value = false;
    }
  }

  function copyDeveloperAddress() {
    copyAddressToClipboard(developerAddress.value);
  }

  return {
    showCelebration,
    showTipModal,
    tipAmount,
    tipPrivateKey,
    tipLoading,
    developerAddress,
    tipAmountOptions,
    tipMode,
    showQRCode,
    showPrivateKeyInput,
    qrCodeDataURL,
    tipWalletBalance,
    tipBalanceSufficient,
    shouldShowTipWalletStatus,
    generateQRCode,
    switchTipMode,
    copyAddressToClipboard,
    triggerCelebration,
    skipTip,
    sendTip,
    copyDeveloperAddress,
    queryTipWalletBalance,
  };
}
