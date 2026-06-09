import { ethers } from 'ethers';
import { useDataOperations as useSharedDataOperations } from '@/features/common/composables/useDataOperations';

/**
 * EVM-specific data operations - delegates to shared composable.
 */
export function useDataOperations(options = {}) {
  return useSharedDataOperations({
    ...options,
    validatePrivateKey(privateKey) {
      try {
        if (!privateKey || typeof privateKey !== 'string') return false;
        let cleanKey = privateKey.trim();
        if (cleanKey.startsWith('0x') || cleanKey.startsWith('0X')) cleanKey = cleanKey.slice(2);
        if (cleanKey.length !== 64) return false;
        if (!/^[0-9a-fA-F]{64}$/.test(cleanKey)) return false;
        new ethers.Wallet(privateKey);
        return true;
      } catch {
        return false;
      }
    },
    validateAddress(address) {
      try {
        if (!address || typeof address !== 'string') return false;
        const trimmedAddress = address.trim();
        if (!trimmedAddress.startsWith('0x') || trimmedAddress.length !== 42) return false;
        const hexPart = trimmedAddress.slice(2);
        if (!/^[0-9a-fA-F]{40}$/.test(hexPart)) return false;
        return ethers.isAddress(trimmedAddress);
      } catch {
        return false;
      }
    },
    deriveAddress(privateKey) {
      const wallet = new ethers.Wallet(privateKey);
      return wallet.address;
    },
  });
}
