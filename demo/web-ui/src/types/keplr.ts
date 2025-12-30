/**
 * Keplr wallet TypeScript types
 * Based on @keplr-wallet/types but simplified for our use case
 */

import type { OfflineSigner } from '@cosmjs/proto-signing';

export interface Bech32Config {
  bech32PrefixAccAddr: string;
  bech32PrefixAccPub: string;
  bech32PrefixValAddr: string;
  bech32PrefixValPub: string;
  bech32PrefixConsAddr: string;
  bech32PrefixConsPub: string;
}

export interface Currency {
  coinDenom: string;
  coinMinimalDenom: string;
  coinDecimals: number;
  coinGeckoId?: string;
  coinImageUrl?: string;
  gasPriceStep?: {
    low: number;
    average: number;
    high: number;
  };
}

export interface ChainInfo {
  chainId: string;
  chainName: string;
  rpc: string;
  rest: string;
  bip44: {
    coinType: number;
  };
  bech32Config: Bech32Config;
  currencies: Currency[];
  feeCurrencies: Currency[];
  stakeCurrency: Currency;
  features?: string[];
}

export interface Key {
  name: string;
  algo: string;
  pubKey: Uint8Array;
  address: Uint8Array;
  bech32Address: string;
  isNanoLedger: boolean;
  isKeystone?: boolean;
}

export interface Keplr {
  /**
   * Check if Keplr is available and enabled
   */
  enable(chainId: string): Promise<void>;

  /**
   * Suggest a chain to Keplr (adds it to the wallet)
   */
  experimentalSuggestChain(chainInfo: ChainInfo): Promise<void>;

  /**
   * Get the offline signer for a chain
   */
  getOfflineSigner(chainId: string): OfflineSigner;

  /**
   * Get the offline signer with amino support
   */
  getOfflineSignerOnlyAmino(chainId: string): OfflineSigner;

  /**
   * Get the key (address, pubkey) for a chain
   */
  getKey(chainId: string): Promise<Key>;

  /**
   * Sign arbitrary data (for off-chain signatures)
   */
  signArbitrary(
    chainId: string,
    signer: string,
    data: string | Uint8Array
  ): Promise<{
    signature: string;
    pub_key: {
      type: string;
      value: string;
    };
  }>;
}

declare global {
  interface Window {
    keplr?: Keplr;
  }
}
