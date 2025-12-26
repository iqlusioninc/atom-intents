import { useMemo } from 'react';
import { useStore } from '../hooks/useStore';
import {
  Trophy,
  Zap,
  ArrowRight,
  CheckCircle,
  Activity,
  Target
} from 'lucide-react';
import type { Solver, SolverType, Settlement, SolverQuote } from '../types';

// Solver type configuration
const solverTypeConfig: Record<SolverType, { label: string; color: string; icon: string; description: string }> = {
  dex_router: {
    label: 'DEX Router',
    color: 'bg-blue-600',
    icon: '🔄',
    description: 'Routes through on-chain DEXes'
  },
  intent_matcher: {
    label: 'Matcher',
    color: 'bg-green-600',
    icon: '🎯',
    description: 'Matches opposing intents directly'
  },
  cex_backstop: {
    label: 'CEX Backstop',
    color: 'bg-purple-600',
    icon: '🏦',
    description: 'Uses CEX liquidity for large orders'
  },
  hybrid: {
    label: 'Hybrid',
    color: 'bg-orange-600',
    icon: '⚡',
    description: 'Cross-chain bridge + settlement'
  },
};

// Calculate solver stats from settlements
function useSolverStats() {
  const solvers = useStore((state) => Array.from(state.solvers.values()));
  const settlements = useStore((state) => Array.from(state.settlements.values()));
  const quotes = useStore((state) => state.quotes);

  return useMemo(() => {
    // Count wins per solver from completed settlements
    const winsByolver: Record<string, number> = {};
    const volumeBySolver: Record<string, number> = {};

    settlements.forEach((s: Settlement) => {
      if (s.status === 'completed') {
        winsByolver[s.solver_id] = (winsByolver[s.solver_id] || 0) + 1;
        volumeBySolver[s.solver_id] = (volumeBySolver[s.solver_id] || 0) + s.input_amount;
      }
    });

    // Determine "best for" based on supported denoms and solver type
    const bestFor: Record<string, string[]> = {};
    solvers.forEach((solver: Solver) => {
      const pairs: string[] = [];
      if (solver.solver_type === 'intent_matcher') {
        pairs.push('Direct Matches');
      } else if (solver.solver_type === 'cex_backstop') {
        pairs.push('Large Orders');
      } else if (solver.solver_type === 'hybrid' && solver.supported_denoms.includes('TIA')) {
        pairs.push('TIA Swaps');
      } else if (solver.supported_denoms.includes('OSMO')) {
        pairs.push('ATOM/OSMO');
      } else if (solver.supported_denoms.includes('NTRN')) {
        pairs.push('NTRN Pairs');
      }
      bestFor[solver.id] = pairs;
    });

    // Sort by wins, then by success rate
    const rankedSolvers = [...solvers].sort((a, b) => {
      const aWins = winsByolver[a.id] || 0;
      const bWins = winsByolver[b.id] || 0;
      if (bWins !== aWins) return bWins - aWins;
      return b.success_rate - a.success_rate;
    });

    return {
      rankedSolvers,
      winsByolver,
      volumeBySolver,
      bestFor,
      recentQuotes: quotes.slice(-10).reverse(),
      completedSettlements: settlements
        .filter((s: Settlement) => s.status === 'completed')
        .sort((a, b) => new Date(b.completed_at || b.updated_at).getTime() - new Date(a.completed_at || a.updated_at).getTime())
        .slice(0, 5),
    };
  }, [solvers, settlements, quotes]);
}

// Leaderboard row component
function LeaderboardRow({
  solver,
  rank,
  wins,
  bestFor
}: {
  solver: Solver;
  rank: number;
  wins: number;
  bestFor: string[];
}) {
  const config = solverTypeConfig[solver.solver_type];
  const isTopThree = rank <= 3;
  const medalColors = ['text-yellow-400', 'text-gray-300', 'text-amber-600'];

  return (
    <div className={`flex items-center gap-4 p-4 rounded-lg transition-colors ${
      isTopThree ? 'bg-gray-800/70' : 'bg-gray-800/30 hover:bg-gray-800/50'
    }`}>
      {/* Rank */}
      <div className={`w-8 h-8 flex items-center justify-center font-bold ${
        isTopThree ? medalColors[rank - 1] : 'text-gray-500'
      }`}>
        {isTopThree ? (
          <Trophy className="w-5 h-5" />
        ) : (
          <span>{rank}</span>
        )}
      </div>

      {/* Solver info */}
      <div className={`w-10 h-10 rounded-lg ${config.color} flex items-center justify-center text-xl`}>
        {config.icon}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="font-medium text-white truncate">{solver.name}</span>
          <span className={`px-1.5 py-0.5 text-xs rounded ${
            solver.status === 'active' ? 'bg-green-900/50 text-green-400' : 'bg-gray-700 text-gray-400'
          }`}>
            {solver.status}
          </span>
        </div>
        <div className="flex items-center gap-2 mt-0.5">
          <span className="text-xs text-gray-500">{config.label}</span>
          {bestFor.length > 0 && (
            <>
              <span className="text-gray-600">•</span>
              <span className="text-xs text-cosmos-400">{bestFor[0]}</span>
            </>
          )}
        </div>
      </div>

      {/* Stats */}
      <div className="flex items-center gap-6 text-sm">
        <div className="text-center">
          <p className="text-white font-semibold">{wins}</p>
          <p className="text-xs text-gray-500">wins</p>
        </div>
        <div className="text-center">
          <p className="text-green-400 font-semibold">{(solver.success_rate * 100).toFixed(0)}%</p>
          <p className="text-xs text-gray-500">success</p>
        </div>
        <div className="text-center hidden sm:block">
          <p className="text-gray-300 font-semibold">{solver.avg_execution_time_ms}ms</p>
          <p className="text-xs text-gray-500">avg time</p>
        </div>
      </div>
    </div>
  );
}

// Activity feed item
function ActivityItem({
  type,
  solver,
  details,
  timestamp
}: {
  type: 'quote' | 'win' | 'settlement';
  solver: { name: string; type: SolverType };
  details: string;
  timestamp: string;
}) {
  const config = solverTypeConfig[solver.type];
  const icons = {
    quote: <Zap className="w-3 h-3 text-yellow-400" />,
    win: <Trophy className="w-3 h-3 text-green-400" />,
    settlement: <CheckCircle className="w-3 h-3 text-blue-400" />,
  };

  const timeAgo = useMemo(() => {
    const diff = Date.now() - new Date(timestamp).getTime();
    if (diff < 60000) return 'just now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    return `${Math.floor(diff / 3600000)}h ago`;
  }, [timestamp]);

  return (
    <div className="flex items-center gap-3 py-2 border-b border-gray-800 last:border-0">
      <div className={`w-6 h-6 rounded ${config.color} flex items-center justify-center text-xs`}>
        {config.icon}
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-sm text-gray-300 truncate">
          <span className="text-white font-medium">{solver.name}</span>
          {' '}{details}
        </p>
      </div>
      <div className="flex items-center gap-2">
        {icons[type]}
        <span className="text-xs text-gray-500">{timeAgo}</span>
      </div>
    </div>
  );
}

export default function SolverDashboard() {
  const solvers = useStore((state) => Array.from(state.solvers.values()));
  const { rankedSolvers, winsByolver, bestFor, recentQuotes, completedSettlements } = useSolverStats();

  const totalWins = Object.values(winsByolver).reduce((a, b) => a + b, 0);
  const activeSolvers = solvers.filter((s) => s.status === 'active').length;

  // Build activity feed from quotes and settlements
  const activityFeed = useMemo(() => {
    const items: Array<{
      type: 'quote' | 'win' | 'settlement';
      solver: { name: string; type: SolverType };
      details: string;
      timestamp: string;
    }> = [];

    // Add recent quotes
    recentQuotes.forEach((q: SolverQuote) => {
      items.push({
        type: 'quote',
        solver: { name: q.solver_name, type: q.solver_type },
        details: `submitted quote for ${(q.input_amount / 1_000_000).toFixed(1)} tokens`,
        timestamp: q.submitted_at,
      });
    });

    // Add completed settlements as wins
    completedSettlements.forEach((s: Settlement) => {
      const solver = solvers.find((sol) => sol.id === s.solver_id);
      if (solver) {
        items.push({
          type: 'win',
          solver: { name: solver.name, type: solver.solver_type },
          details: `won auction and settled ${(s.input_amount / 1_000_000).toFixed(1)} tokens`,
          timestamp: s.completed_at || s.updated_at,
        });
      }
    });

    return items
      .sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())
      .slice(0, 8);
  }, [recentQuotes, completedSettlements, solvers]);

  return (
    <div className="space-y-6 animate-slide-in">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold text-white">Solver Leaderboard</h2>
          <p className="text-gray-400">Real-time competition between solvers</p>
        </div>
        <div className="flex items-center gap-4">
          <div className="text-right">
            <p className="text-2xl font-bold text-white">{activeSolvers}</p>
            <p className="text-xs text-gray-500">active solvers</p>
          </div>
          <div className="text-right">
            <p className="text-2xl font-bold text-green-400">{totalWins}</p>
            <p className="text-xs text-gray-500">total wins</p>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Leaderboard - takes 2 columns */}
        <div className="lg:col-span-2 space-y-2">
          <div className="flex items-center gap-2 mb-4">
            <Trophy className="w-5 h-5 text-cosmos-400" />
            <h3 className="font-semibold text-white">Rankings</h3>
          </div>

          <div className="space-y-2">
            {rankedSolvers.map((solver, index) => (
              <LeaderboardRow
                key={solver.id}
                solver={solver}
                rank={index + 1}
                wins={winsByolver[solver.id] || 0}
                bestFor={bestFor[solver.id] || []}
              />
            ))}
          </div>
        </div>

        {/* Activity Feed - takes 1 column */}
        <div className="card">
          <div className="flex items-center gap-2 mb-4">
            <Activity className="w-5 h-5 text-cosmos-400" />
            <h3 className="font-semibold text-white">Live Activity</h3>
          </div>

          {activityFeed.length > 0 ? (
            <div className="space-y-1">
              {activityFeed.map((item, index) => (
                <ActivityItem key={index} {...item} />
              ))}
            </div>
          ) : (
            <div className="text-center py-8 text-gray-500">
              <Activity className="w-8 h-8 mx-auto mb-2 opacity-50" />
              <p className="text-sm">No recent activity</p>
              <p className="text-xs mt-1">Submit an intent to see solvers compete</p>
            </div>
          )}
        </div>
      </div>

      {/* Solver Types Explanation - Compact */}
      <div className="card bg-gray-900/50">
        <div className="flex items-center gap-2 mb-4">
          <Target className="w-5 h-5 text-cosmos-400" />
          <h3 className="font-semibold text-white">Solver Strategies</h3>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          {Object.entries(solverTypeConfig).map(([type, config]) => (
            <div key={type} className="flex items-start gap-3">
              <div className={`w-8 h-8 rounded-lg ${config.color} flex items-center justify-center text-lg flex-shrink-0`}>
                {config.icon}
              </div>
              <div>
                <p className="text-sm font-medium text-white">{config.label}</p>
                <p className="text-xs text-gray-500">{config.description}</p>
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* How it works - very compact */}
      <div className="flex items-center justify-center gap-4 text-sm text-gray-500 py-4">
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-yellow-500"></span>
          Intent submitted
        </span>
        <ArrowRight className="w-4 h-4" />
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-blue-500"></span>
          Solvers compete
        </span>
        <ArrowRight className="w-4 h-4" />
        <span className="flex items-center gap-1">
          <span className="w-2 h-2 rounded-full bg-green-500"></span>
          Best quote wins
        </span>
      </div>
    </div>
  );
}
