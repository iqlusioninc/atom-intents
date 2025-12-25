import { useCallback } from 'react';
import { Routes, Route, Link, useLocation } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  Zap,
  BarChart3,
  Users,
  Layers,
  Settings,
  WifiOff,
  Calculator,
  ExternalLink,
  Github,
} from 'lucide-react';

import { useWebSocket } from './hooks/useWebSocket';
import { useStore } from './hooks/useStore';
import * as api from './services/api';

import Dashboard from './components/Dashboard';
import IntentCreator from './components/IntentCreator';
import AuctionView from './components/AuctionView';
import SolverDashboard from './components/SolverDashboard';
import SettlementMonitor from './components/SettlementMonitor';
import DemoScenarios from './components/DemoScenarios';
import WalletConnect from './components/WalletConnect';
import CostCalculator from './components/CostCalculator';

// Cosmos Hub Logo SVG component - atom orbital design
function CosmosLogo({ className = "w-8 h-8" }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="16" cy="16" r="14" stroke="url(#cosmos-gradient)" strokeWidth="1.5" fill="none" opacity="0.3" />
      <circle cx="16" cy="16" r="3.5" fill="url(#cosmos-gradient)" />
      <ellipse cx="16" cy="16" rx="11" ry="4" stroke="url(#cosmos-gradient)" strokeWidth="1.5" fill="none" transform="rotate(0 16 16)" opacity="0.7" />
      <ellipse cx="16" cy="16" rx="11" ry="4" stroke="url(#cosmos-gradient)" strokeWidth="1.5" fill="none" transform="rotate(60 16 16)" opacity="0.7" />
      <ellipse cx="16" cy="16" rx="11" ry="4" stroke="url(#cosmos-gradient)" strokeWidth="1.5" fill="none" transform="rotate(-60 16 16)" opacity="0.7" />
      <defs>
        <linearGradient id="cosmos-gradient" x1="0" y1="0" x2="32" y2="32">
          <stop offset="0%" stopColor="#a78bfa" />
          <stop offset="50%" stopColor="#8b5cf6" />
          <stop offset="100%" stopColor="#6d28d9" />
        </linearGradient>
      </defs>
    </svg>
  );
}

function App() {
  const location = useLocation();
  const {
    addIntent,
    addAuction,
    updateAuction,
    addSettlement,
    updateSettlement,
    setPrices,
    setSolvers,
    addQuote,
    clearQuotes,
    setStats,
  } = useStore();

  // WebSocket handlers
  const handleIntent = useCallback((intent: Parameters<typeof addIntent>[0]) => {
    addIntent(intent);
  }, [addIntent]);

  const handleAuction = useCallback((auction: Parameters<typeof addAuction>[0]) => {
    if (auction.status === 'open') {
      addAuction(auction);
      clearQuotes();
    } else {
      updateAuction(auction.id, auction);
    }
  }, [addAuction, updateAuction, clearQuotes]);

  const handleQuote = useCallback((quote: Parameters<typeof addQuote>[0]) => {
    addQuote(quote);
  }, [addQuote]);

  const handleSettlement = useCallback((settlement: Parameters<typeof addSettlement>[0]) => {
    const existing = useStore.getState().settlements.get(settlement.id);
    if (existing) {
      updateSettlement(settlement.id, settlement);
    } else {
      addSettlement(settlement);
    }
  }, [addSettlement, updateSettlement]);

  const handlePrices = useCallback((prices: Parameters<typeof setPrices>[0]) => {
    setPrices(prices);
  }, [setPrices]);

  const handleStats = useCallback((stats: Parameters<typeof setStats>[0]) => {
    setStats(stats);
  }, [setStats]);

  const { connected } = useWebSocket(
    handleIntent,
    handleAuction,
    handleQuote,
    handleSettlement,
    handlePrices,
    handleStats
  );

  // Initial data fetch
  useQuery({
    queryKey: ['intents'],
    queryFn: async () => {
      const data = await api.listIntents();
      data.intents.forEach(addIntent);
      return data;
    },
  });

  useQuery({
    queryKey: ['solvers'],
    queryFn: async () => {
      const data = await api.listSolvers();
      setSolvers(data.solvers);
      return data;
    },
  });

  useQuery({
    queryKey: ['prices'],
    queryFn: async () => {
      const data = await api.getPrices();
      setPrices(data.prices);
      return data;
    },
  });

  useQuery({
    queryKey: ['stats'],
    queryFn: async () => {
      const data = await api.getStats();
      setStats(data);
      return data;
    },
  });

  const navItems = [
    { path: '/', icon: Activity, label: 'Dashboard' },
    { path: '/create', icon: Zap, label: 'Create Intent' },
    { path: '/auction', icon: BarChart3, label: 'Auctions' },
    { path: '/solvers', icon: Users, label: 'Solvers' },
    { path: '/settlements', icon: Layers, label: 'Settlements' },
    { path: '/calculator', icon: Calculator, label: 'Cost Calculator' },
    { path: '/demo', icon: Settings, label: 'Demo Scenarios' },
  ];

  return (
    <div className="min-h-screen bg-space-950 bg-mesh">
      {/* Header */}
      <header className="sticky top-0 z-50 border-b border-white/5 bg-space-950/80 backdrop-blur-xl">
        <div className="max-w-[1600px] mx-auto px-6">
          <div className="flex items-center justify-between h-16">
            {/* Logo and title */}
            <div className="flex items-center gap-4">
              <div className="relative">
                <CosmosLogo className="w-9 h-9" />
                <div className="absolute inset-0 blur-xl bg-cosmos-500/30 -z-10" />
              </div>
              <div>
                <h1 className="text-lg font-semibold text-white tracking-tight">
                  ATOM Intents
                </h1>
                <p className="text-xs text-space-400 -mt-0.5">
                  Intent-Based Liquidity for Cosmos Hub
                </p>
              </div>
            </div>

            {/* Right side controls */}
            <div className="flex items-center gap-3">
              {/* Connection status */}
              <div className={`flex items-center gap-2 px-3 py-1.5 rounded-full text-xs font-medium transition-all ${
                connected
                  ? 'bg-atom-green/10 text-atom-green border border-atom-green/20'
                  : 'bg-red-500/10 text-red-400 border border-red-500/20'
              }`}>
                {connected ? (
                  <>
                    <span className="relative flex h-2 w-2">
                      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-atom-green opacity-75"></span>
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-atom-green"></span>
                    </span>
                    <span>Live</span>
                  </>
                ) : (
                  <>
                    <WifiOff className="w-3.5 h-3.5" />
                    <span>Offline</span>
                  </>
                )}
              </div>

              {/* Wallet connect */}
              <WalletConnect />

              {/* GitHub link */}
              <a
                href="https://github.com/iqlusioninc/atom-intents"
                target="_blank"
                rel="noopener noreferrer"
                className="p-2 rounded-xl text-space-400 hover:text-white hover:bg-white/5 transition-all"
                title="View on GitHub"
              >
                <Github className="w-5 h-5" />
              </a>
            </div>
          </div>
        </div>
      </header>

      <div className="flex">
        {/* Sidebar Navigation */}
        <nav className="w-64 min-h-[calc(100vh-4rem)] border-r border-white/5 bg-space-950/50 backdrop-blur-sm relative">
          <div className="p-4 space-y-1">
            {navItems.map(({ path, icon: Icon, label }) => {
              const isActive = location.pathname === path;
              return (
                <Link
                  key={path}
                  to={path}
                  className={isActive ? 'nav-item-active' : 'nav-item'}
                >
                  <Icon className="w-5 h-5" />
                  <span className="font-medium">{label}</span>
                </Link>
              );
            })}
          </div>

          {/* Bottom info section */}
          <div className="absolute bottom-0 left-0 right-0 p-4 border-t border-white/5">
            <div className="p-4 rounded-xl bg-space-900/60 border border-white/5">
              <p className="text-xs font-medium text-space-300 mb-1">Demo Environment</p>
              <p className="text-xs text-space-500 leading-relaxed">
                This is a simulation. No real transactions are executed.
              </p>
              <a
                href="https://github.com/iqlusioninc/atom-intents/blob/main/spec/SPECIFICATION.md"
                target="_blank"
                rel="noopener noreferrer"
                className="mt-3 flex items-center gap-1.5 text-xs text-cosmos-400 hover:text-cosmos-300 transition-colors"
              >
                <span>Read Specification</span>
                <ExternalLink className="w-3 h-3" />
              </a>
            </div>
          </div>
        </nav>

        {/* Main Content */}
        <main className="flex-1 p-6 overflow-auto">
          <div className="max-w-6xl mx-auto">
            <Routes>
              <Route path="/" element={<Dashboard />} />
              <Route path="/create" element={<IntentCreator />} />
              <Route path="/auction" element={<AuctionView />} />
              <Route path="/solvers" element={<SolverDashboard />} />
              <Route path="/settlements" element={<SettlementMonitor />} />
              <Route path="/calculator" element={<CostCalculator />} />
              <Route path="/demo" element={<DemoScenarios />} />
            </Routes>
          </div>
        </main>
      </div>
    </div>
  );
}

export default App;
