import { ethers } from 'ethers'
import { Keypair, PublicKey } from '@solana/web3.js'
import bs58 from 'bs58'

/**
 * Ecosystem-specific configurations for EVM and Solana chains.
 *
 * Property names match the parameters of shared composables (useDataOperations, useBalanceQuery)
 * so they can be spread directly into composable options.
 */
export const ECOSYSTEMS = {
  evm: {
    // ── Commands used by shared composables ──
    queryCommand: 'query_balances_with_updates',
    stopCommand: 'stop_balance_query',
    resetCommand: 'reset_balance_query_stop',
    baseTransferCommand: 'base_coin_transfer',
    tokenTransferCommand: 'token_transfer',
    // ── Validation limits ──
    precisionMax: 18,
    supportsGasLimit: true,
    supportsGasPrice: true,
    supportsPriorityFee: false,
    // ── Validators (matches useDataOperations options) ──
    validatePrivateKey(privateKey) {
      try {
        if (!privateKey || typeof privateKey !== 'string') return false
        let cleanKey = privateKey.trim()
        if (cleanKey.startsWith('0x') || cleanKey.startsWith('0X')) cleanKey = cleanKey.slice(2)
        if (cleanKey.length !== 64) return false
        if (!/^[0-9a-fA-F]{64}$/.test(cleanKey)) return false
        new ethers.Wallet(privateKey)
        return true
      } catch {
        return false
      }
    },
    validateAddress(address) {
      try {
        if (!address || typeof address !== 'string') return false
        const trimmedAddress = address.trim()
        if (!trimmedAddress.startsWith('0x') || trimmedAddress.length !== 42) return false
        const hexPart = trimmedAddress.slice(2)
        if (!/^[0-9a-fA-F]{40}$/.test(hexPart)) return false
        return ethers.isAddress(trimmedAddress)
      } catch {
        return false
      }
    },
    deriveAddress(privateKey) {
      const wallet = new ethers.Wallet(privateKey)
      return wallet.address
    },
    // ── Tip ──
    developerAddress: '0x298E1bE50Ba5f50CF23cFA6b5F1dF347cFBef40A',
  },

  solana: {
    queryCommand: 'sol_query_balances_with_updates',
    stopCommand: 'stop_balance_query',
    resetCommand: 'reset_balance_query_stop',
    baseTransferCommand: 'sol_transfer',
    tokenTransferCommand: 'sol_token_transfer',
    precisionMax: 9,
    supportsGasLimit: false,
    supportsGasPrice: false,
    supportsPriorityFee: true,
    validatePrivateKey(privateKey) {
      try {
        if (!privateKey || typeof privateKey !== 'string') return false
        const cleanKey = privateKey.trim()
        const secretKey = bs58.decode(cleanKey)
        if (secretKey.length !== 64) return false
        Keypair.fromSecretKey(secretKey)
        return true
      } catch {
        return false
      }
    },
    validateAddress(address) {
      try {
        if (!address || typeof address !== 'string') return false
        const trimmedAddress = address.trim()
        new PublicKey(trimmedAddress)
        return true
      } catch {
        return false
      }
    },
    deriveAddress(privateKey) {
      const secretKey = bs58.decode(privateKey.trim())
      const keypair = Keypair.fromSecretKey(secretKey)
      return keypair.publicKey.toBase58()
    },
    developerAddress: '7EcDhSYGxXyscszYEp35KHN8vvw3svAuLKTzXwCFLtV',
  },
}

export function getEcosystem(key) {
  if (key === 'sol' || key === 'solana') return ECOSYSTEMS.solana
  return ECOSYSTEMS.evm
}
