import { useStore } from '../hooks/useStore';
import { Activity, Clock, Trophy, TrendingUp } from 'lucide-react';
import type { Solver, SolverType } from '../types';

function SolverCard({ solver }: { solver: Solver }) {
  const statusConfig = {
    active: { label: 'Active', color: 'bg-green-500', dotClass: 'status-dot-active' },
    idle: { label: 'Idle', color: 'bg-yellow-500', dotClass: 'status-dot-pending' },
    suspended: { label: 'Suspended', color: 'bg-red-500', dotClass: 'status-dot-error' },
    offline: { label: 'Offline', color: 'bg-gray-500', dotClass: 'status-dot' },
  };

  const typeConfig: Record<SolverType, { label: string; color: string; icon: string }> = {
    dex_router: { label: 'DEX Router', color: 'bg-blue-600', icon: '🔄' },
    intent_matcher: { label: 'Intent Matcher', color: 'bg-green-600', icon: '🎯' },
    cex_backstop: { label: 'CEX Backstop', color: 'bg-purple-600', icon: '🏦' },
    hybrid: { label: 'Hybrid', color: 'bg-orange-600', icon: '⚡' },
  };

  const { label: statusLabel, dotClass } = statusConfig[solver.status];
  const { label: typeLabel, color: typeColor, icon: typeIcon } = typeConfig[solver.solver_type];

  return (
    <div className="card hover:border-cosmos-500/50 transition-colors">
      <div className="flex items-start justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className={`w-12 h-12 rounded-lg ${typeColor} flex items-center justify-center text-2xl`}>
            {typeIcon}
          </div>
          <div>
            <h3 className="font-semibold text-white">{solver.name}</h3>
            <p className="text-xs text-gray-400">{typeLabel}</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span className={dotClass} />
          <span className="text-sm text-gray-400">{statusLabel}</span>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4 mb-4">
        <div className="p-3 bg-gray-800/50 rounded-lg">
          <div className="flex items-center gap-2 text-gray-400 text-xs mb-1">
            <Trophy className="w-3 h-3" />
            <span>Reputation</span>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex-1 bg-gray-700 rounded-full h-2">
              <div
                className="bg-cosmos-500 h-2 rounded-full transition-all"
                style={{ width: `${solver.reputation_score * 100}%` }}
              />
            </div>
            <span className="text-white text-sm font-medium">
              {(solver.reputation_score * 100).toFixed(0)}%
            </span>
          </div>
        </div>

        <div className="p-3 bg-gray-800/50 rounded-lg">
          <div className="flex items-center gap-2 text-gray-400 text-xs mb-1">
            <Activity className="w-3 h-3" />
            <span>Success Rate</span>
          </div>
          <p className="text-white font-medium">
            {(solver.success_rate * 100).toFixed(1)}%
          </p>
        </div>

        <div className="p-3 bg-gray-800/50 rounded-lg">
          <div className="flex items-center gap-2 text-gray-400 text-xs mb-1">
            <Clock className="w-3 h-3" />
            <span>Avg Execution</span>
          </div>
          <p className="text-white font-medium">{solver.avg_execution_time_ms}ms</p>
        </div>

        <div className="p-3 bg-gray-800/50 rounded-lg">
          <div className="flex items-center gap-2 text-gray-400 text-xs mb-1">
            <TrendingUp className="w-3 h-3" />
            <span>Total Volume</span>
          </div>
          <p className="text-white font-medium">
            ${(solver.total_volume / 1_000_000_000).toFixed(2)}B
          </p>
        </div>
      </div>

      <div className="pt-4 border-t border-gray-700">
        <p className="text-xs text-gray-400 mb-2">Supported Assets</p>
        <div className="flex flex-wrap gap-2">
          {solver.supported_denoms.map((denom) => (
            <span
              key={denom}
              className="px-2 py-1 bg-gray-800 rounded text-xs text-gray-300"
            >
              {denom}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function SolverDashboard() {
  const solvers = useStore((state) => Array.from(state.solvers.values()));

  const activeSolvers = solvers.filter((s) => s.status === 'active').length;
  const avgReputation =
    solvers.reduce((sum, s) => sum + s.reputation_score, 0) / solvers.length || 0;
  const avgSuccessRate =
    solvers.reduce((sum, s) => sum + s.success_rate, 0) / solvers.length || 0;
  const totalVolume = solvers.reduce((sum, s) => sum + s.total_volume, 0);

  return (
    <div className="space-y-6 animate-slide-in">
      <div>
        <h2 className="text-2xl font-bold text-white">Solver Dashboard</h2>
        <p className="text-gray-400">Monitor solver performance and competition</p>
      </div>

      {/* Overview Stats */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <div className="card">
          <p className="text-gray-400 text-sm">Active Solvers</p>
          <p className="text-2xl font-bold text-white">{activeSolvers}</p>
          <p className="text-xs text-gray-500">of {solvers.length} total</p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Avg Reputation</p>
          <p className="text-2xl font-bold text-white">
            {(avgReputation * 100).toFixed(0)}%
          </p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Avg Success Rate</p>
          <p className="text-2xl font-bold text-green-400">
            {(avgSuccessRate * 100).toFixed(1)}%
          </p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Total Volume</p>
          <p className="text-2xl font-bold text-white">
            ${(totalVolume / 1_000_000_000).toFixed(2)}B
          </p>
        </div>
      </div>

      {/* Solver Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {solvers.map((solver) => (
          <SolverCard key={solver.id} solver={solver} />
        ))}
      </div>

      {/* How Solvers Work */}
      <div className="card bg-cosmos-900/20 border-cosmos-700/50">
        <h3 className="text-lg font-semibold text-white mb-4">How Solvers Compete</h3>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          <div>
            <div className="w-10 h-10 rounded-lg bg-blue-600 flex items-center justify-center mb-3">
              🔄
            </div>
            <h4 className="font-medium text-white mb-1">DEX Routers</h4>
            <p className="text-sm text-gray-400">
              Route through on-chain DEXes like Osmosis. Zero capital required, aggregates
              liquidity across chains.
            </p>
          </div>
          <div>
            <div className="w-10 h-10 rounded-lg bg-green-600 flex items-center justify-center mb-3">
              🎯
            </div>
            <h4 className="font-medium text-white mb-1">Intent Matchers</h4>
            <p className="text-sm text-gray-400">
              Match opposing intents directly. Best prices with zero capital through
              coincidence of wants.
            </p>
          </div>
          <div>
            <div className="w-10 h-10 rounded-lg bg-purple-600 flex items-center justify-center mb-3">
              🏦
            </div>
            <h4 className="font-medium text-white mb-1">CEX Backstop</h4>
            <p className="text-sm text-gray-400">
              Hedge against CEX prices for deep liquidity. Used for large orders when DEX
              liquidity is insufficient.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
