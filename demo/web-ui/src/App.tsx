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
  Wifi,
  WifiOff,
  Calculator,
  HelpCircle,
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
import OnboardingWizard, { useOnboardingWizard } from './components/OnboardingWizard';

function App() {
  const location = useLocation();
  const { isOpen: isWizardOpen, openWizard, closeWizard } = useOnboardingWizard();
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
    <div className="min-h-screen bg-gradient-to-br from-gray-900 via-gray-900 to-cosmos-950">
      {/* Header */}
      <header className="border-b border-gray-800 bg-gray-900/80 backdrop-blur-sm sticky top-0 z-50">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex items-center justify-between h-16">
            <div className="flex items-center gap-4">
              <span className="text-2xl">⚛️</span>
              <div>
                <h1 className="text-xl font-bold text-white">ATOM Intents Demo</h1>
                <p className="text-xs text-gray-400">Intent-Based Liquidity System</p>
              </div>
            </div>

            <div className="flex items-center gap-4">
              <button
                onClick={openWizard}
                className="flex items-center gap-2 px-3 py-1.5 text-sm text-gray-400 hover:text-white hover:bg-gray-800 rounded-lg transition-colors"
                title="Show guide"
              >
                <HelpCircle className="w-4 h-4" />
                <span className="hidden sm:inline">Guide</span>
              </button>
              <div className={`flex items-center gap-2 px-3 py-1 rounded-full text-sm ${
                connected ? 'bg-green-900/30 text-green-400' : 'bg-red-900/30 text-red-400'
              }`}>
                {connected ? (
                  <>
                    <Wifi className="w-4 h-4" />
                    <span>Live</span>
                  </>
                ) : (
                  <>
                    <WifiOff className="w-4 h-4" />
                    <span>Offline</span>
                  </>
                )}
              </div>
              <WalletConnect />
            </div>
          </div>
        </div>
      </header>

      <div className="flex">
        {/* Sidebar */}
        <nav className="w-64 min-h-[calc(100vh-4rem)] border-r border-gray-800 bg-gray-900/50 p-4">
          <ul className="space-y-2">
            {navItems.map(({ path, icon: Icon, label }) => (
              <li key={path}>
                <Link
                  to={path}
                  className={`flex items-center gap-3 px-4 py-2 rounded-lg transition-colors ${
                    location.pathname === path
                      ? 'bg-cosmos-600 text-white'
                      : 'text-gray-400 hover:bg-gray-800 hover:text-white'
                  }`}
                >
                  <Icon className="w-5 h-5" />
                  <span>{label}</span>
                </Link>
              </li>
            ))}
          </ul>
        </nav>

        {/* Main Content */}
        <main className="flex-1 p-6">
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

      {/* Onboarding Wizard */}
      <OnboardingWizard isOpen={isWizardOpen} onClose={closeWizard} />
    </div>
  );
}

export default App;
