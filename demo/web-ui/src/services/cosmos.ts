/**
 * Cosmos blockchain service
 * Handles Keplr wallet connection, balance queries, and contract execution
 */

import { SigningCosmWasmClient } from '@cosmjs/cosmwasm-stargate';
import { StargateClient } from '@cosmjs/stargate';
import { GasPrice } from '@cosmjs/stargate';
import type { Coin } from '@cosmjs/stargate';
import { providerTestnet, contracts } from '../config/chains';
import type { ChainInfo, Keplr, Key } from '../types/keplr';

/**
 * Check if Keplr is installed
 */
export function isKeplrInstalled(): boolean {
  return typeof window !== 'undefined' && window.keplr !== undefined;
}

/**
 * Get Keplr instance
 */
export function getKeplr(): Keplr | undefined {
  return window.keplr;
}

/**
 * Wait for Keplr to be available (with timeout)
 */
export async function waitForKeplr(timeout = 3000): Promise<Keplr | null> {
  if (isKeplrInstalled()) {
    return window.keplr!;
  }

  return new Promise((resolve) => {
    const start = Date.now();
    const interval = setInterval(() => {
      if (isKeplrInstalled()) {
        clearInterval(interval);
        resolve(window.keplr!);
      } else if (Date.now() - start > timeout) {
        clearInterval(interval);
        resolve(null);
      }
    }, 100);
  });
}

/**
 * Suggest the provider testnet chain to Keplr
 */
export async function suggestChain(chainInfo: ChainInfo = providerTestnet): Promise<void> {
  const keplr = getKeplr();
  if (!keplr) {
    throw new Error('Keplr not installed');
  }
  await keplr.experimentalSuggestChain(chainInfo);
}

/**
 * Enable Keplr for the provider testnet
 */
export async function enableKeplr(chainId: string = providerTestnet.chainId): Promise<void> {
  const keplr = getKeplr();
  if (!keplr) {
    throw new Error('Keplr not installed');
  }
  await keplr.enable(chainId);
}

/**
 * Get the user's key (address) from Keplr
 */
export async function getKey(chainId: string = providerTestnet.chainId): Promise<Key> {
  const keplr = getKeplr();
  if (!keplr) {
    throw new Error('Keplr not installed');
  }
  return keplr.getKey(chainId);
}

/**
 * Create a read-only Stargate client for balance queries
 */
export async function createQueryClient(): Promise<StargateClient> {
  return StargateClient.connect(providerTestnet.rpc);
}

/**
 * Create a signing CosmWasm client for contract execution
 */
export async function createSigningClient(
  chainId: string = providerTestnet.chainId
): Promise<SigningCosmWasmClient> {
  const keplr = getKeplr();
  if (!keplr) {
    throw new Error('Keplr not installed');
  }

  const offlineSigner = keplr.getOfflineSigner(chainId);
  const gasPrice = GasPrice.fromString('0.025uatom');

  return SigningCosmWasmClient.connectWithSigner(
    providerTestnet.rpc,
    offlineSigner,
    { gasPrice }
  );
}

/**
 * Get balance for an address
 */
export async function getBalance(
  address: string,
  denom: string = 'uatom'
): Promise<Coin> {
  const client = await createQueryClient();
  const balance = await client.getBalance(address, denom);
  client.disconnect();
  return balance;
}

/**
 * Get all balances for an address
 */
export async function getAllBalances(address: string): Promise<readonly Coin[]> {
  const client = await createQueryClient();
  const balances = await client.getAllBalances(address);
  client.disconnect();
  return balances;
}

/**
 * Escrow deposit message type
 */
export interface EscrowLockMsg {
  lock: {
    escrow_id: string;
    intent_id: string;
    expires_at: number;
  };
}

/**
 * Lock funds in escrow contract
 * This is called when user creates an intent
 */
export async function lockInEscrow(
  senderAddress: string,
  amount: string,
  denom: string,
  escrowId: string,
  intentId: string,
  timeoutSeconds: number = 600
): Promise<{
  transactionHash: string;
  height: number;
  gasUsed: bigint;
}> {
  const client = await createSigningClient();

  // Calculate expiration timestamp (current time + timeout in nanoseconds)
  const expiresAt = Math.floor(Date.now() / 1000) + timeoutSeconds;

  const msg: EscrowLockMsg = {
    lock: {
      escrow_id: escrowId,
      intent_id: intentId,
      expires_at: expiresAt,
    },
  };

  const funds: Coin[] = [{ denom, amount }];

  const result = await client.execute(
    senderAddress,
    contracts.escrow,
    msg,
    'auto',
    `Lock funds for intent ${intentId}`,
    funds
  );

  client.disconnect();

  return {
    transactionHash: result.transactionHash,
    height: result.height,
    gasUsed: result.gasUsed,
  };
}

/**
 * Convert uatom to ATOM (divide by 10^6)
 */
export function fromMicroDenom(amount: string | number, decimals: number = 6): number {
  const value = typeof amount === 'string' ? parseInt(amount, 10) : amount;
  return value / Math.pow(10, decimals);
}

/**
 * Convert ATOM to uatom (multiply by 10^6)
 */
export function toMicroDenom(amount: number, decimals: number = 6): string {
  return Math.floor(amount * Math.pow(10, decimals)).toString();
}

/**
 * Format address for display (cosmos1abc...xyz)
 */
export function formatAddress(address: string, prefixLen: number = 10, suffixLen: number = 6): string {
  if (address.length <= prefixLen + suffixLen) {
    return address;
  }
  return `${address.slice(0, prefixLen)}...${address.slice(-suffixLen)}`;
}

/**
 * Connection result type
 */
export interface WalletConnection {
  address: string;
  name: string;
  balance: Coin;
  client: SigningCosmWasmClient;
}

/**
 * Full wallet connection flow
 * 1. Check Keplr installed
 * 2. Suggest chain
 * 3. Enable chain
 * 4. Get address
 * 5. Create signing client
 * 6. Get balance
 */
export async function connectWallet(): Promise<WalletConnection> {
  // Check if Keplr is installed
  const keplr = await waitForKeplr();
  if (!keplr) {
    throw new Error('Please install Keplr wallet extension');
  }

  // Suggest the provider testnet chain
  await suggestChain(providerTestnet);

  // Enable the chain (prompts user if first time)
  await enableKeplr(providerTestnet.chainId);

  // Get the key (address and name)
  const key = await getKey(providerTestnet.chainId);

  // Create signing client
  const client = await createSigningClient(providerTestnet.chainId);

  // Get balance
  const balance = await getBalance(key.bech32Address);

  return {
    address: key.bech32Address,
    name: key.name,
    balance,
    client,
  };
}

/**
 * Disconnect wallet (clean up clients)
 */
export async function disconnectWallet(client: SigningCosmWasmClient | null): Promise<void> {
  if (client) {
    client.disconnect();
  }
}
