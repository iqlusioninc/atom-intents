import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { SigningCosmWasmClient } from '@cosmjs/cosmwasm-stargate';
import {
  connectWallet,
  disconnectWallet,
  getAllBalances,
  lockInEscrow,
  isKeplrInstalled,
  fromMicroDenom,
  toMicroDenom,
} from '../services/cosmos';

export type WalletType = 'keplr' | 'demo';
export type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

interface WalletState {
  // Connection state
  connected: boolean;
  status: ConnectionStatus;
  walletType: WalletType | null;
  error: string | null;

  // User info
  address: string | null;
  name: string | null;
  balances: Record<string, number>; // In micro denom (e.g., uatom)
  balanceLoading: boolean; // True when fetching balances

  // Signing client (not persisted)
  client: SigningCosmWasmClient | null;

  // Transaction state
  pendingTx: boolean;
  lastTxHash: string | null;

  // Actions
  connect: (walletType: WalletType) => Promise<void>;
  disconnect: () => Promise<void>;
  refreshBalance: () => Promise<void>;
  lockFundsInEscrow: (
    amount: number,
    denom: string,
    intentId: string
  ) => Promise<{ txHash: string; height: number }>;

  // Helpers
  getBalanceFormatted: (denom: string) => number;
  hasEnoughBalance: (denom: string, amount: number) => boolean;
  isKeplrAvailable: () => boolean;
}

// Generate a random cosmos address for demo mode
function generateDemoAddress(): string {
  const chars = '0123456789abcdef';
  let suffix = '';
  for (let i = 0; i < 38; i++) {
    suffix += chars[Math.floor(Math.random() * chars.length)];
  }
  return `cosmos1${suffix}`;
}

// Generate demo balances
function generateDemoBalances(): Record<string, number> {
  return {
    ATOM: Math.floor(Math.random() * 1000 + 100) * 1_000_000,
    OSMO: Math.floor(Math.random() * 5000 + 500) * 1_000_000,
    USDC: Math.floor(Math.random() * 10000 + 1000) * 1_000_000,
    NTRN: Math.floor(Math.random() * 2000 + 200) * 1_000_000,
    STRD: Math.floor(Math.random() * 500 + 50) * 1_000_000,
  };
}

export const useWallet = create<WalletState>()(
  persist(
    (set, get) => ({
      // Initial state
      connected: false,
      status: 'disconnected',
      walletType: null,
      error: null,
      address: null,
      name: null,
      balances: {},
      balanceLoading: false,
      client: null,
      pendingTx: false,
      lastTxHash: null,

      connect: async (walletType: WalletType) => {
        set({ status: 'connecting', error: null });

        try {
          if (walletType === 'keplr') {
            // Real Keplr connection
            const connection = await connectWallet();

            set({
              connected: true,
              status: 'connected',
              walletType: 'keplr',
              address: connection.address,
              name: connection.name,
              balances: {
                ATOM: parseInt(connection.balance.amount, 10),
              },
              client: connection.client,
              error: null,
            });

            // Fetch all balances in background
            get().refreshBalance();
          } else {
            // Demo mode with simulated data
            await new Promise((resolve) => setTimeout(resolve, 500));

            set({
              connected: true,
              status: 'connected',
              walletType: 'demo',
              address: generateDemoAddress(),
              name: 'Demo Wallet',
              balances: generateDemoBalances(),
              client: null,
              error: null,
            });
          }
        } catch (err) {
          const errorMessage = err instanceof Error ? err.message : 'Failed to connect wallet';
          set({
            connected: false,
            status: 'error',
            error: errorMessage,
          });
          throw err;
        }
      },

      disconnect: async () => {
        const { client } = get();
        await disconnectWallet(client);

        set({
          connected: false,
          status: 'disconnected',
          walletType: null,
          address: null,
          name: null,
          balances: {},
          client: null,
          error: null,
          pendingTx: false,
          lastTxHash: null,
        });
      },

      refreshBalance: async () => {
        const { address, walletType } = get();
        if (!address || walletType !== 'keplr') return;

        set({ balanceLoading: true });
        try {
          const allBalances = await getAllBalances(address);

          const balances: Record<string, number> = {};
          for (const coin of allBalances) {
            // Map denom to display name
            const denomMap: Record<string, string> = {
              uatom: 'ATOM',
              uosmo: 'OSMO',
              untrn: 'NTRN',
              ustrd: 'STRD',
            };
            const displayDenom = denomMap[coin.denom] || coin.denom;
            balances[displayDenom] = parseInt(coin.amount, 10);
          }

          set({ balances, balanceLoading: false });
        } catch (err) {
          console.error('Failed to refresh balance:', err);
          set({ balanceLoading: false });
        }
      },

      lockFundsInEscrow: async (
        amount: number,
        denom: string,
        intentId: string
      ): Promise<{ txHash: string; height: number }> => {
        const { address, walletType, client } = get();

        if (!address) {
          throw new Error('Wallet not connected');
        }

        if (walletType === 'demo') {
          // Simulate transaction for demo mode
          set({ pendingTx: true });
          await new Promise((resolve) => setTimeout(resolve, 2000));
          const fakeTxHash = `DEMO_${Date.now().toString(16).toUpperCase()}`;
          set({ pendingTx: false, lastTxHash: fakeTxHash });
          return { txHash: fakeTxHash, height: 12345678 };
        }

        if (!client) {
          throw new Error('Signing client not available');
        }

        set({ pendingTx: true, error: null });

        try {
          // Convert amount to micro denom
          const microAmount = toMicroDenom(amount);
          const microDenom = denom === 'ATOM' ? 'uatom' : `u${denom.toLowerCase()}`;

          // Use intentId as both escrow_id and intent_id
          const escrowId = `escrow_${intentId}`;

          const result = await lockInEscrow(
            address,
            microAmount,
            microDenom,
            escrowId,
            intentId,
            600 // 10 minute timeout
          );

          set({
            pendingTx: false,
            lastTxHash: result.transactionHash,
          });

          // Refresh balance after transaction
          get().refreshBalance();

          return {
            txHash: result.transactionHash,
            height: result.height,
          };
        } catch (err) {
          const errorMessage = err instanceof Error ? err.message : 'Transaction failed';
          set({ pendingTx: false, error: errorMessage });
          throw err;
        }
      },

      getBalanceFormatted: (denom: string): number => {
        const { balances } = get();
        const microBalance = balances[denom] || 0;
        return fromMicroDenom(microBalance);
      },

      hasEnoughBalance: (denom: string, amount: number): boolean => {
        const { balances } = get();
        const microBalance = balances[denom] || 0;
        const microAmount = toMicroDenom(amount);
        return microBalance >= parseInt(microAmount, 10);
      },

      isKeplrAvailable: (): boolean => {
        return isKeplrInstalled();
      },
    }),
    {
      name: 'atom-intents-wallet',
      // Don't persist the signing client
      partialize: (state) => ({
        connected: state.connected,
        walletType: state.walletType,
        address: state.address,
        name: state.name,
        // Don't persist balances - always refresh on reconnect
      }),
      // After rehydration, refresh balances and restore client for Keplr users
      onRehydrateStorage: () => (state) => {
        if (state?.connected && state?.walletType === 'keplr' && state?.address) {
          // Use setTimeout to ensure store is fully initialized
          setTimeout(async () => {
            useWallet.setState({ balanceLoading: true });

            // Retry helper with exponential backoff
            const retryWithBackoff = async <T>(
              fn: () => Promise<T>,
              maxRetries: number = 3
            ): Promise<T> => {
              let lastError: unknown;
              for (let attempt = 0; attempt < maxRetries; attempt++) {
                try {
                  return await fn();
                } catch (err) {
                  lastError = err;
                  if (attempt < maxRetries - 1) {
                    const delay = Math.min(1000 * Math.pow(2, attempt), 5000);
                    await new Promise(r => setTimeout(r, delay));
                  }
                }
              }
              throw lastError;
            };

            try {
              // First, just refresh balances (doesn't require user interaction)
              const { getAllBalances, createSigningClient } = await import('../services/cosmos');
              const allBalances = await retryWithBackoff(() => getAllBalances(state.address!));

              const denomMap: Record<string, string> = {
                uatom: 'ATOM',
                uosmo: 'OSMO',
                untrn: 'NTRN',
                ustrd: 'STRD',
              };

              const balances: Record<string, number> = {};
              for (const coin of allBalances) {
                const displayDenom = denomMap[coin.denom] || coin.denom;
                balances[displayDenom] = parseInt(coin.amount, 10);
              }

              useWallet.setState({ balances, balanceLoading: false });

              // Try to restore signing client (may require Keplr approval)
              try {
                const client = await createSigningClient();
                useWallet.setState({ client });
              } catch (clientErr) {
                // Client restoration failed - user will need to reconnect for signing
                console.warn('Could not restore signing client:', clientErr);
              }
            } catch (err) {
              console.error('Failed to refresh balances on rehydration:', err);
              useWallet.setState({ balanceLoading: false });
            }
          }, 100);
        }
      },
    }
  )
);
