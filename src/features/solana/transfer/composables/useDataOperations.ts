import { Keypair, PublicKey } from '@solana/web3.js';
import bs58 from 'bs58';
import { useDataOperations as useSharedDataOperations } from '@/features/common/composables/useDataOperations';

/**
 * Solana-specific data operations - delegates to shared composable.
 */
export function useDataOperations(options = {}) {
  return useSharedDataOperations({
    ...options,
    validatePrivateKey(privateKey) {
      try {
        if (!privateKey || typeof privateKey !== 'string') return false;
        const cleanKey = privateKey.trim();
        const secretKey = bs58.decode(cleanKey);
        if (secretKey.length !== 64) return false;
        Keypair.fromSecretKey(secretKey);
        return true;
      } catch {
        return false;
      }
    },
    validateAddress(address) {
      try {
        if (!address || typeof address !== 'string') return false;
        const trimmedAddress = address.trim();
        const pubKey = new PublicKey(trimmedAddress);
        return PublicKey.isOnCurve(pubKey.toBytes());
      } catch {
        return false;
      }
    },
    deriveAddress(privateKey) {
      const secretKey = bs58.decode(privateKey.trim());
      const keypair = Keypair.fromSecretKey(secretKey);
      return keypair.publicKey.toBase58();
    },
  });
}
