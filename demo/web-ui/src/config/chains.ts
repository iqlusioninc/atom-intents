/**
 * Chain configuration for Cosmos testnets
 * Used for Keplr wallet integration
 */

import type { ChainInfo } from '../types/keplr';

/**
 * Provider testnet (Cosmos Hub ICS testnet)
 * This is where the escrow and settlement contracts are deployed
 */
export const providerTestnet: ChainInfo = {
  chainId: 'provider',
  chainName: 'Cosmos Hub Testnet (Provider)',
  rpc: 'https://cosmos-testnet-rpc.polkachu.com',
  rest: 'https://cosmos-testnet-api.polkachu.com',
  bip44: {
    coinType: 118,
  },
  bech32Config: {
    bech32PrefixAccAddr: 'cosmos',
    bech32PrefixAccPub: 'cosmospub',
    bech32PrefixValAddr: 'cosmosvaloper',
    bech32PrefixValPub: 'cosmosvaloperpub',
    bech32PrefixConsAddr: 'cosmosvalcons',
    bech32PrefixConsPub: 'cosmosvalconspub',
  },
  currencies: [
    {
      coinDenom: 'ATOM',
      coinMinimalDenom: 'uatom',
      coinDecimals: 6,
      coinGeckoId: 'cosmos',
    },
  ],
  feeCurrencies: [
    {
      coinDenom: 'ATOM',
      coinMinimalDenom: 'uatom',
      coinDecimals: 6,
      coinGeckoId: 'cosmos',
      gasPriceStep: {
        low: 0.01,
        average: 0.025,
        high: 0.04,
      },
    },
  ],
  stakeCurrency: {
    coinDenom: 'ATOM',
    coinMinimalDenom: 'uatom',
    coinDecimals: 6,
    coinGeckoId: 'cosmos',
  },
  features: ['cosmwasm'],
};

/**
 * Contract addresses on provider testnet
 */
export const contracts = {
  escrow: 'cosmos13jv2umdqvlkfncpd6vf7r2sc0ljdtenmzujlpqqpgagarassqsws86phq9',
  settlement: 'cosmos1xwft7w6kcspzufftw6ky4f5e8sykumpuenpm34tkxk4epmya0jdsahgsff',
} as const;

/**
 * Testnet faucet URL
 */
export const TESTNET_FAUCET_URL = 'https://faucet.polkachu.com/cosmos-testnet';

/**
 * Block explorer URL for provider testnet
 */
export const EXPLORER_URL = 'https://testnet.mintscan.io/cosmos-testnet';

/**
 * Get transaction URL for block explorer
 */
export function getTxUrl(txHash: string): string {
  return `${EXPLORER_URL}/tx/${txHash}`;
}

/**
 * Get address URL for block explorer
 */
export function getAddressUrl(address: string): string {
  return `${EXPLORER_URL}/address/${address}`;
}
