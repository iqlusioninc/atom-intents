import { create } from 'zustand';
import type { Intent, Auction, Settlement, PriceFeed, Solver, SystemStats, SolverQuote } from '../types';

interface AppState {
  // Data
  intents: Map<string, Intent>;
  auctions: Map<string, Auction>;
  settlements: Map<string, Settlement>;
  prices: Map<string, PriceFeed>;
  solvers: Map<string, Solver>;
  quotes: SolverQuote[];
  stats: SystemStats | null;

  // UI state
  currentAuctionId: string | null;
  selectedIntentId: string | null;
  selectedSettlementId: string | null;

  // Actions
  addIntent: (intent: Intent) => void;
  updateIntent: (id: string, updates: Partial<Intent>) => void;
  addAuction: (auction: Auction) => void;
  updateAuction: (id: string, updates: Partial<Auction>) => void;
  addSettlement: (settlement: Settlement) => void;
  updateSettlement: (id: string, updates: Partial<Settlement>) => void;
  setPrices: (prices: PriceFeed[]) => void;
  setSolvers: (solvers: Solver[]) => void;
  addQuote: (quote: SolverQuote) => void;
  clearQuotes: () => void;
  setStats: (stats: SystemStats) => void;
  setCurrentAuctionId: (id: string | null) => void;
  setSelectedIntentId: (id: string | null) => void;
  setSelectedSettlementId: (id: string | null) => void;
}

export const useStore = create<AppState>((set) => ({
  // Initial data
  intents: new Map(),
  auctions: new Map(),
  settlements: new Map(),
  prices: new Map(),
  solvers: new Map(),
  quotes: [],
  stats: null,

  // Initial UI state
  currentAuctionId: null,
  selectedIntentId: null,
  selectedSettlementId: null,

  // Actions
  addIntent: (intent) =>
    set((state) => ({
      intents: new Map(state.intents).set(intent.id, intent),
    })),

  updateIntent: (id, updates) =>
    set((state) => {
      const intents = new Map(state.intents);
      const existing = intents.get(id);
      if (existing) {
        intents.set(id, { ...existing, ...updates });
      }
      return { intents };
    }),

  addAuction: (auction) =>
    set((state) => ({
      auctions: new Map(state.auctions).set(auction.id, auction),
      currentAuctionId: auction.id,
    })),

  updateAuction: (id, updates) =>
    set((state) => {
      const auctions = new Map(state.auctions);
      const existing = auctions.get(id);
      if (existing) {
        auctions.set(id, { ...existing, ...updates });
      }
      return { auctions };
    }),

  addSettlement: (settlement) =>
    set((state) => ({
      settlements: new Map(state.settlements).set(settlement.id, settlement),
    })),

  updateSettlement: (id, updates) =>
    set((state) => {
      const settlements = new Map(state.settlements);
      const existing = settlements.get(id);
      if (existing) {
        settlements.set(id, { ...existing, ...updates });
      }
      return { settlements };
    }),

  setPrices: (prices) =>
    set(() => ({
      prices: new Map(prices.map((p) => [p.denom, p])),
    })),

  setSolvers: (solvers) =>
    set(() => ({
      solvers: new Map(solvers.map((s) => [s.id, s])),
    })),

  addQuote: (quote) =>
    set((state) => ({
      quotes: [...state.quotes, quote],
    })),

  clearQuotes: () =>
    set(() => ({
      quotes: [],
    })),

  setStats: (stats) =>
    set(() => ({
      stats,
    })),

  setCurrentAuctionId: (id) =>
    set(() => ({
      currentAuctionId: id,
    })),

  setSelectedIntentId: (id) =>
    set(() => ({
      selectedIntentId: id,
    })),

  setSelectedSettlementId: (id) =>
    set(() => ({
      selectedSettlementId: id,
    })),
}));
