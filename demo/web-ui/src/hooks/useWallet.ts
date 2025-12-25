import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface WalletState {
  connected: boolean;
  address: string | null;
  name: string | null;
  balances: Record<string, number>;

  // Actions
  connect: (walletType: 'keplr' | 'leap' | 'demo') => Promise<void>;
  disconnect: () => void;
  updateBalance: (denom: string, amount: number) => void;
}

// Generate a random cosmos address
function generateDemoAddress(): string {
  const chars = '0123456789abcdef';
  let suffix = '';
  for (let i = 0; i < 38; i++) {
    suffix += chars[Math.floor(Math.random() * chars.length)];
  }
  return `cosmos1${suffix}`;
}

export const useWallet = create<WalletState>()(
  persist(
    (set, get) => ({
      connected: false,
      address: null,
      name: null,
      balances: {},

      connect: async (walletType) => {
        // Simulate wallet connection
        await new Promise((resolve) => setTimeout(resolve, 500));

        let address: string;
        let name: string;

        switch (walletType) {
          case 'keplr':
            // In a real app, this would use window.keplr
            address = generateDemoAddress();
            name = 'Keplr Wallet';
            break;
          case 'leap':
            address = generateDemoAddress();
            name = 'Leap Wallet';
            break;
          case 'demo':
          default:
            address = generateDemoAddress();
            name = 'Demo Wallet';
            break;
        }

        // Set demo balances
        const balances: Record<string, number> = {
          ATOM: Math.floor(Math.random() * 1000 + 100) * 1_000_000,
          OSMO: Math.floor(Math.random() * 5000 + 500) * 1_000_000,
          USDC: Math.floor(Math.random() * 10000 + 1000) * 1_000_000,
          NTRN: Math.floor(Math.random() * 2000 + 200) * 1_000_000,
          STRD: Math.floor(Math.random() * 500 + 50) * 1_000_000,
        };

        set({
          connected: true,
          address,
          name,
          balances,
        });
      },

      disconnect: () => {
        set({
          connected: false,
          address: null,
          name: null,
          balances: {},
        });
      },

      updateBalance: (denom, amount) => {
        const { balances } = get();
        set({
          balances: {
            ...balances,
            [denom]: amount,
          },
        });
      },
    }),
    {
      name: 'atom-intents-wallet',
    }
  )
);
