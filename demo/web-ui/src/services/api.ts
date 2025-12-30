// API service for communicating with Skip Select Simulator

import type {
  Intent,
  Auction,
  Settlement,
  PriceFeed,
  Solver,
  SystemStats,
  CreateIntentRequest,
  SolverQuote,
} from '../types';

const API_BASE = '/api/v1';

async function fetchJson<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  });

  if (!response.ok) {
    const error = await response.text();
    throw new Error(`API Error: ${response.status} - ${error}`);
  }

  return response.json();
}

// Intents
export async function submitIntent(request: CreateIntentRequest): Promise<{ success: boolean; intent: Intent }> {
  return fetchJson(`${API_BASE}/intents`, {
    method: 'POST',
    body: JSON.stringify(request),
  });
}

export async function listIntents(): Promise<{ intents: Intent[]; total: number }> {
  return fetchJson(`${API_BASE}/intents`);
}

export async function getIntent(id: string): Promise<Intent> {
  return fetchJson(`${API_BASE}/intents/${id}`);
}

// Auctions
export async function getCurrentAuction(): Promise<Auction> {
  return fetchJson(`${API_BASE}/auctions/current`);
}

export async function getAuction(id: string): Promise<Auction> {
  return fetchJson(`${API_BASE}/auctions/${id}`);
}

export async function getAuctionQuotes(id: string): Promise<{ auction_id: string; quotes: SolverQuote[]; total: number }> {
  return fetchJson(`${API_BASE}/auctions/${id}/quotes`);
}

// Settlements
export async function getSettlement(id: string): Promise<Settlement> {
  return fetchJson(`${API_BASE}/settlements/${id}`);
}

// Prices
export async function getPrices(): Promise<{ prices: PriceFeed[]; updated_at: string }> {
  return fetchJson(`${API_BASE}/prices`);
}

// Solvers
export async function listSolvers(): Promise<{ solvers: Solver[]; total: number }> {
  return fetchJson(`${API_BASE}/solvers`);
}

// Stats
export async function getStats(): Promise<SystemStats> {
  return fetchJson(`${API_BASE}/stats`);
}

// Demo endpoints
export async function generateDemoIntent(): Promise<{ intent: Intent; description: string }> {
  return fetchJson(`${API_BASE}/demo/generate-intent`, { method: 'POST' });
}

export async function runScenario(name: string): Promise<{
  scenario: string;
  description: string;
  intents_created: number;
  intent_ids: string[];
}> {
  return fetchJson(`${API_BASE}/demo/scenario/${name}`, { method: 'POST' });
}

// Health check
export async function healthCheck(): Promise<{ status: string; version: string; uptime_seconds: number }> {
  return fetchJson('/health');
}

// Chain health
export interface ChainHealthStatus {
  chain_id: string;
  healthy: boolean;
  latest_height: number | null;
  synced: boolean;
  rpc_url: string;
  error: string | null;
}

export interface ChainHealthResponse {
  chains: ChainHealthStatus[];
  all_healthy: boolean;
  mode: string;
}

export async function getChainHealth(): Promise<ChainHealthResponse> {
  return fetchJson(`${API_BASE}/chains/health`);
}

// Wallet status (admin)
export interface WalletBalance {
  denom: string;
  amount: string;
  amount_display: string;
}

export interface WalletStatus {
  address: string;
  balances: WalletBalance[];
  low_balance_warning: boolean;
  chain_id: string;
}

export interface WalletStatusResponse {
  wallet: WalletStatus | null;
  mode: string;
  warnings: string[];
}

export async function getWalletStatus(): Promise<WalletStatusResponse> {
  return fetchJson(`${API_BASE}/admin/wallet`);
}
