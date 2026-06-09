import { useBalanceQuery as useSharedBalanceQuery } from '@/features/common/composables/useBalanceQuery';

/**
 * EVM-specific balance query - delegates to shared composable.
 */
export function useBalanceQuery(options = {}) {
  return useSharedBalanceQuery(options, {
    queryCommand: 'query_balances_with_updates',
    stopCommand: 'stop_balance_query',
    resetCommand: 'reset_balance_query_stop',
  });
}
