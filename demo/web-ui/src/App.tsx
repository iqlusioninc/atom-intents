import { useCallback, useState } from 'react';
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
  Menu,
  X,
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
import OnboardingWizard, { useOnboardingWizard } from './components/OnboardingWizard';

function App() {
  const location = useLocation();
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
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
          <div className="flex items-center justify-between h-14 sm:h-16">
            {/* Mobile menu button */}
            <button
              onClick={() => setIsMobileMenuOpen(!isMobileMenuOpen)}
              className="lg:hidden p-2 -ml-2 text-gray-400 hover:text-white hover:bg-gray-800 rounded-lg transition-colors"
              aria-label="Toggle menu"
            >
              {isMobileMenuOpen ? <X className="w-6 h-6" /> : <Menu className="w-6 h-6" />}
            </button>

            <div className="flex items-center gap-2 sm:gap-4">
              <span className="text-xl sm:text-2xl">⚛️</span>
              <div>
                <h1 className="text-base sm:text-xl font-bold text-white">ATOM Intents</h1>
                <p className="text-xs text-gray-400 hidden sm:block">Intent-Based Liquidity System</p>
              </div>
            </div>

            <div className="flex items-center gap-2 sm:gap-4">
              <a
                href="https://github.com/iqlusioninc/atom-intents"
                target="_blank"
                rel="noopener noreferrer"
                className="hidden sm:flex items-center gap-2 px-3 py-1.5 text-sm text-gray-400 hover:text-white hover:bg-gray-800 rounded-lg transition-colors"
                title="View on GitHub"
              >
                <Github className="w-4 h-4" />
                <span className="hidden md:inline">GitHub</span>
              </a>
              <button
                onClick={openWizard}
                className="hidden sm:flex items-center gap-2 px-3 py-1.5 text-sm text-gray-400 hover:text-white hover:bg-gray-800 rounded-lg transition-colors"
                title="Show guide"
              >
                <HelpCircle className="w-4 h-4" />
                <span className="hidden md:inline">Guide</span>
              </button>
              <div className={`hidden sm:flex items-center gap-2 px-2 sm:px-3 py-1 rounded-full text-xs sm:text-sm ${
                connected ? 'bg-green-900/30 text-green-400' : 'bg-red-900/30 text-red-400'
              }`}>
                {connected ? (
                  <>
                    <Wifi className="w-4 h-4" />
                    <span className="hidden sm:inline">Live</span>
                  </>
                ) : (
                  <>
                    <WifiOff className="w-4 h-4" />
                    <span className="hidden sm:inline">Offline</span>
                  </>
                )}
              </div>
              <WalletConnect />
            </div>
          </div>
        </div>
      </header>

      <div className="flex relative">
        {/* Mobile menu overlay */}
        {isMobileMenuOpen && (
          <div
            className="fixed inset-0 bg-black/50 z-30 lg:hidden"
            onClick={() => setIsMobileMenuOpen(false)}
          />
        )}

        {/* Sidebar - hidden on mobile, visible as overlay when menu is open */}
        <nav
          className={`
            fixed lg:relative z-40 lg:z-auto
            w-64 min-h-[calc(100vh-3.5rem)] sm:min-h-[calc(100vh-4rem)]
            border-r border-gray-800 bg-gray-900 lg:bg-gray-900/50 p-4
            transform transition-transform duration-200 ease-in-out
            ${isMobileMenuOpen ? 'translate-x-0' : '-translate-x-full lg:translate-x-0'}
          `}
        >
          {/* Mobile-only header in sidebar */}
          <div className="lg:hidden flex items-center justify-between mb-4 pb-4 border-b border-gray-800">
            <div className="flex items-center gap-2">
              <span className="text-xl">⚛️</span>
              <span className="font-semibold text-white">Menu</span>
            </div>
            <div className={`flex items-center gap-1 px-2 py-0.5 rounded-full text-xs ${
              connected ? 'bg-green-900/30 text-green-400' : 'bg-red-900/30 text-red-400'
            }`}>
              {connected ? <Wifi className="w-3 h-3" /> : <WifiOff className="w-3 h-3" />}
              <span>{connected ? 'Live' : 'Offline'}</span>
            </div>
          </div>

          <ul className="space-y-1 sm:space-y-2">
            {navItems.map(({ path, icon: Icon, label }) => (
              <li key={path}>
                <Link
                  to={path}
                  onClick={() => setIsMobileMenuOpen(false)}
                  className={`flex items-center gap-3 px-3 sm:px-4 py-2.5 sm:py-2 rounded-lg transition-colors ${
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

          {/* Mobile-only guide and GitHub buttons */}
          <div className="lg:hidden mt-4 pt-4 border-t border-gray-800 space-y-1">
            <a
              href="https://github.com/iqlusioninc/atom-intents"
              target="_blank"
              rel="noopener noreferrer"
              onClick={() => setIsMobileMenuOpen(false)}
              className="flex items-center gap-3 px-3 py-2.5 w-full text-gray-400 hover:bg-gray-800 hover:text-white rounded-lg transition-colors"
            >
              <Github className="w-5 h-5" />
              <span>GitHub</span>
            </a>
            <button
              onClick={() => {
                openWizard();
                setIsMobileMenuOpen(false);
              }}
              className="flex items-center gap-3 px-3 py-2.5 w-full text-gray-400 hover:bg-gray-800 hover:text-white rounded-lg transition-colors"
            >
              <HelpCircle className="w-5 h-5" />
              <span>Show Guide</span>
            </button>
          </div>
        </nav>

        {/* Main Content */}
        <main className="flex-1 p-3 sm:p-4 lg:p-6 min-w-0">
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
