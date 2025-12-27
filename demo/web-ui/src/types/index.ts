// API Types - matching the Rust models

export interface Asset {
  chain_id: string;
  denom: string;
  amount: number;
}

export interface OutputSpec {
  chain_id: string;
  denom: string;
  min_amount: number;
  max_price?: number;
}

export interface FillConfig {
  allow_partial: boolean;
  min_fill_percent: number;
  strategy: 'eager' | 'all_or_nothing' | 'time_based' | 'price_based';
}

export interface ExecutionConstraints {
  max_hops: number;
  allowed_venues: string[];
  excluded_venues: string[];
  max_slippage_bps: number;
}

export interface Intent {
  id: string;
  user_address: string;
  input: Asset;
  output: OutputSpec;
  fill_config: FillConfig;
  constraints: ExecutionConstraints;
  status: IntentStatus;
  created_at: string;
  expires_at: string;
  auction_id?: string;
  settlement_id?: string;
  /** Amount of input that has been filled (for partial fills) */
  filled_amount?: number;
  /** Remaining amount to be filled */
  remaining_amount?: number;
  /** Fill percentage (0-100) */
  fill_percentage?: number;
}

export type IntentStatus =
  | 'pending'
  | 'in_auction'
  | 'matched'
  | 'partially_filled'
  | 'settling'
  | 'completed'
  | 'failed'
  | 'expired'
  | 'cancelled';

export interface CreateIntentRequest {
  user_address: string;
  input: Asset;
  output: OutputSpec;
  fill_config?: FillConfig;
  constraints?: ExecutionConstraints;
  timeout_seconds?: number;
}

export interface Auction {
  id: string;
  intent_ids: string[];
  status: AuctionStatus;
  quotes: SolverQuote[];
  winning_quote?: SolverQuote;
  clearing_price?: number;
  started_at: string;
  completed_at?: string;
  stats: AuctionStats;
}

export type AuctionStatus =
  | 'open'
  | 'collecting'
  | 'clearing'
  | 'completed'
  | 'failed';

export interface AuctionStats {
  num_intents: number;
  num_quotes: number;
  total_input_amount: number;
  total_output_amount: number;
  matched_volume: number;
  price_improvement_bps: number;
  solver_competition_score: number;
}

export interface SolverQuote {
  id: string;
  solver_id: string;
  solver_name: string;
  solver_type: SolverType;
  intent_ids: string[];
  input_amount: number;
  output_amount: number;
  effective_price: number;
  execution_plan: ExecutionPlan;
  estimated_gas: number;
  confidence: number;
  submitted_at: string;
  /** Reason this solver had an advantage (if any) */
  advantage_reason?: string;
}

export type SolverType =
  | 'dex_router'
  | 'intent_matcher'
  | 'cex_backstop'
  | 'hybrid';

export interface ExecutionPlan {
  plan_type: 'dex_route' | 'direct_match' | 'cex_hedge' | 'multi_hop';
  steps: ExecutionStep[];
  estimated_duration_ms: number;
}

export interface ExecutionStep {
  step_type: string;
  chain_id: string;
  venue?: string;
  input_denom: string;
  output_denom: string;
  amount: number;
  description: string;
}

export interface Settlement {
  id: string;
  auction_id: string;
  intent_ids: string[];
  solver_id: string;
  status: SettlementStatus;
  phase: SettlementPhase;
  input_amount: number;
  output_amount: number;
  escrow_txid?: string;
  execution_txid?: string;
  ibc_packet_id?: string;
  created_at: string;
  updated_at: string;
  completed_at?: string;
  events: SettlementEvent[];
  /** Whether this is a partial fill settlement */
  is_partial_fill?: boolean;
  /** Fill percentage for partial fills (0-100) */
  fill_percentage?: number;
  /** Original requested input amount */
  original_input_amount?: number;
}

export type SettlementStatus =
  | 'pending'
  | 'committing'
  | 'executing'
  | 'completed'
  | 'failed'
  | 'refunded';

export type SettlementPhase =
  | 'init'
  | 'escrow_locked'
  | 'solver_committed'
  | 'ibc_in_flight'
  | 'finalized';

export interface SettlementEvent {
  event_type: string;
  timestamp: string;
  description: string;
  metadata: Record<string, unknown>;
}

export interface PriceFeed {
  denom: string;
  price_usd: number;
  change_24h: number;
  volume_24h: number;
  confidence: number;
  updated_at: string;
}

export interface Solver {
  id: string;
  name: string;
  solver_type: SolverType;
  status: 'active' | 'idle' | 'suspended' | 'offline';
  reputation_score: number;
  total_volume: number;
  success_rate: number;
  avg_execution_time_ms: number;
  supported_chains: string[];
  supported_denoms: string[];
  connected_at?: string;
}

export interface SystemStats {
  total_intents: number;
  total_auctions: number;
  total_settlements: number;
  total_volume_usd: number;
  avg_execution_time_ms: number;
  avg_price_improvement_bps: number;
  success_rate: number;
  active_solvers: number;
  pending_intents: number;
  intents_per_minute: number;
}

// WebSocket message types
export type WsMessage =
  | { type: 'intent_submitted'; data: Intent }
  | { type: 'auction_started'; data: Auction }
  | { type: 'quote_received'; data: SolverQuote }
  | { type: 'auction_completed'; data: Auction }
  | { type: 'settlement_update'; data: Settlement }
  | { type: 'price_update'; data: PriceFeed[] }
  | { type: 'stats_update'; data: SystemStats }
  | { type: 'error'; data: { message: string } }
  | { type: 'pong' };

// Demo scenarios
export const DEMO_SCENARIOS = [
  { id: 'simple_swap', name: 'Simple Swap', description: 'Basic ATOM → OSMO swap via DEX' },
  { id: 'tia_usdc_swap', name: 'TIA → USDC (Celestia)', description: 'Cross-chain swap from Celestia via Hub escrow' },
  { id: 'intent_matching', name: 'Intent Matching', description: 'Two opposing intents matched directly' },
  { id: 'multi_hop', name: 'Multi-Hop', description: 'Cross-chain settlement via IBC PFM' },
  { id: 'cex_backstop', name: 'CEX Backstop', description: 'Large order using CEX liquidity' },
  { id: 'auction_competition', name: 'Auction Competition', description: 'Multiple solvers competing' },
  { id: 'partial_fill', name: 'Partial Fill', description: 'Order accepting 50%+ partial execution' },
  { id: 'lst_swap', name: 'LST Swap', description: 'Liquid staking token swap: stATOM → ATOM' },
] as const;

// Token type for LSM classification
export type TokenType = 'native' | 'lst' | 'stablecoin';

// Token configuration
export const TOKENS: Record<string, {
  symbol: string;
  name: string;
  chain: string;
  logo: string;
  tokenType?: TokenType;
  description?: string;
}> = {
  // Native tokens
  ATOM: { symbol: 'ATOM', name: 'Cosmos Hub', chain: 'cosmoshub-4', logo: '⚛️', tokenType: 'native' },
  OSMO: { symbol: 'OSMO', name: 'Osmosis', chain: 'osmosis-1', logo: '🧪', tokenType: 'native' },
  NTRN: { symbol: 'NTRN', name: 'Neutron', chain: 'neutron-1', logo: '⚡', tokenType: 'native' },
  STRD: { symbol: 'STRD', name: 'Stride', chain: 'stride-1', logo: '🏃', tokenType: 'native' },
  TIA: { symbol: 'TIA', name: 'Celestia', chain: 'celestia', logo: '🟣', tokenType: 'native' },

  // Stablecoins
  USDC: { symbol: 'USDC', name: 'USD Coin', chain: 'noble-1', logo: '💵', tokenType: 'stablecoin' },

  // Liquid Staking Tokens (LSTs) - Fungible tokens from liquid staking providers
  // These are fungible derivatives representing staked assets across multiple validators
  stATOM: {
    symbol: 'stATOM',
    name: 'Stride Staked ATOM',
    chain: 'stride-1',
    logo: '💧',
    tokenType: 'lst',
    description: 'Fungible liquid staking token from Stride representing staked ATOM'
  },
  stOSMO: {
    symbol: 'stOSMO',
    name: 'Stride Staked OSMO',
    chain: 'stride-1',
    logo: '💧',
    tokenType: 'lst',
    description: 'Fungible liquid staking token from Stride representing staked OSMO'
  },
  stTIA: {
    symbol: 'stTIA',
    name: 'Stride Staked TIA',
    chain: 'stride-1',
    logo: '💧',
    tokenType: 'lst',
    description: 'Fungible liquid staking token from Stride representing staked TIA'
  },
  stkATOM: {
    symbol: 'stkATOM',
    name: 'pSTAKE Staked ATOM',
    chain: 'persistence-1',
    logo: '🔒',
    tokenType: 'lst',
    description: 'Fungible liquid staking token from pSTAKE representing staked ATOM'
  },
  qATOM: {
    symbol: 'qATOM',
    name: 'Quicksilver Liquid Staked ATOM',
    chain: 'quicksilver-2',
    logo: '⚡',
    tokenType: 'lst',
    description: 'Fungible liquid staking token from Quicksilver using LSM shares internally'
  },
};

// Note: Native LSM shares (cosmosvaloperxxxx/recordId format) are non-fungible
// and validator-specific. They are created directly on Cosmos Hub via MsgTokenizeShares.
// The LSTs above (stATOM, qATOM, etc.) are fungible derivatives that may use LSM
// shares internally but are abstracted into a single fungible token.

export const CHAINS = [
  { id: 'cosmoshub-4', name: 'Cosmos Hub' },
  { id: 'osmosis-1', name: 'Osmosis' },
  { id: 'neutron-1', name: 'Neutron' },
  { id: 'noble-1', name: 'Noble' },
  { id: 'stride-1', name: 'Stride' },
  { id: 'celestia', name: 'Celestia', hasSmartContracts: false },
  { id: 'persistence-1', name: 'Persistence' },
  { id: 'quicksilver-2', name: 'Quicksilver' },
];
