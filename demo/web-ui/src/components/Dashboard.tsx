import { Activity, TrendingUp, Clock, CheckCircle, Users, DollarSign, ArrowUpRight, ArrowDownRight } from 'lucide-react';
import { useStore } from '../hooks/useStore';
import { formatDistanceToNow } from 'date-fns';

function StatCard({
  icon: Icon,
  label,
  value,
  subValue,
  trend,
  gradient,
}: {
  icon: React.ElementType;
  label: string;
  value: string | number;
  subValue?: string;
  trend?: 'up' | 'down' | null;
  gradient: string;
}) {
  return (
    <div className="card group">
      <div className="flex items-start justify-between">
        <div className={`p-3 rounded-xl bg-gradient-to-br ${gradient}`}>
          <Icon className="w-5 h-5 text-white" />
        </div>
        {trend && (
          <div className={`flex items-center gap-1 text-xs font-medium ${
            trend === 'up' ? 'text-atom-green' : 'text-red-400'
          }`}>
            {trend === 'up' ? (
              <ArrowUpRight className="w-3.5 h-3.5" />
            ) : (
              <ArrowDownRight className="w-3.5 h-3.5" />
            )}
          </div>
        )}
      </div>
      <div className="mt-4">
        <p className="text-space-400 text-sm font-medium">{label}</p>
        <p className="text-2xl font-bold text-white mt-1 tracking-tight tabular-nums">{value}</p>
        {subValue && (
          <p className="text-xs text-space-500 mt-1">{subValue}</p>
        )}
      </div>
    </div>
  );
}

function RecentActivity() {
  const intents = useStore((state) => Array.from(state.intents.values()));
  const recentIntents = intents
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .slice(0, 5);

  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'completed':
        return 'badge-success';
      case 'pending':
      case 'in_auction':
        return 'badge-warning';
      case 'failed':
      case 'expired':
        return 'badge-error';
      default:
        return 'badge-info';
    }
  };

  const getTokenIcon = (denom: string) => {
    switch (denom) {
      case 'ATOM':
        return (
          <div className="w-8 h-8 rounded-full bg-cosmos-500/20 flex items-center justify-center">
            <span className="text-cosmos-400 text-sm font-bold">A</span>
          </div>
        );
      case 'OSMO':
        return (
          <div className="w-8 h-8 rounded-full bg-pink-500/20 flex items-center justify-center">
            <span className="text-pink-400 text-sm font-bold">O</span>
          </div>
        );
      case 'USDC':
        return (
          <div className="w-8 h-8 rounded-full bg-blue-500/20 flex items-center justify-center">
            <span className="text-blue-400 text-sm font-bold">$</span>
          </div>
        );
      default:
        return (
          <div className="w-8 h-8 rounded-full bg-space-700 flex items-center justify-center">
            <span className="text-space-300 text-sm font-bold">{denom[0]}</span>
          </div>
        );
    }
  };

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-6">
        <h3 className="text-lg font-semibold text-white">Recent Activity</h3>
        <span className="text-xs text-space-400">{intents.length} total intents</span>
      </div>
      <div className="space-y-3">
        {recentIntents.length === 0 ? (
          <div className="text-center py-8">
            <Activity className="w-10 h-10 text-space-600 mx-auto mb-3" />
            <p className="text-space-400 text-sm">No recent activity</p>
            <p className="text-space-500 text-xs mt-1">Submit an intent to get started</p>
          </div>
        ) : (
          recentIntents.map((intent) => (
            <div
              key={intent.id}
              className="flex items-center justify-between p-3 rounded-xl bg-space-900/40 hover:bg-space-800/60 border border-transparent hover:border-white/5 transition-all"
            >
              <div className="flex items-center gap-3">
                {getTokenIcon(intent.input.denom)}
                <div>
                  <p className="text-white font-medium text-sm">
                    {(intent.input.amount / 1_000_000).toFixed(2)} {intent.input.denom}
                    <span className="text-space-500 mx-2">→</span>
                    {intent.output.denom}
                  </p>
                  <p className="text-xs text-space-500">
                    {formatDistanceToNow(new Date(intent.created_at), { addSuffix: true })}
                  </p>
                </div>
              </div>
              <span className={getStatusBadge(intent.status)}>{intent.status}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function PriceTicker() {
  const prices = useStore((state) => Array.from(state.prices.values()));

  const getTokenGradient = (denom: string) => {
    switch (denom) {
      case 'ATOM':
        return 'from-cosmos-500/20 to-cosmos-600/20';
      case 'OSMO':
        return 'from-pink-500/20 to-pink-600/20';
      case 'USDC':
        return 'from-blue-500/20 to-blue-600/20';
      case 'NTRN':
        return 'from-orange-500/20 to-orange-600/20';
      default:
        return 'from-space-700/50 to-space-800/50';
    }
  };

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-6">
        <h3 className="text-lg font-semibold text-white">Live Prices</h3>
        <div className="flex items-center gap-1.5">
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-atom-green opacity-75"></span>
            <span className="relative inline-flex rounded-full h-2 w-2 bg-atom-green"></span>
          </span>
          <span className="text-xs text-space-400">Live</span>
        </div>
      </div>
      <div className="space-y-2">
        {prices.map((price) => (
          <div
            key={price.denom}
            className={`flex items-center justify-between p-3 rounded-xl bg-gradient-to-r ${getTokenGradient(price.denom)} border border-white/5`}
          >
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-full bg-white/10 flex items-center justify-center">
                <span className="text-white text-sm font-bold">{price.denom[0]}</span>
              </div>
              <span className="text-white font-medium">{price.denom}</span>
            </div>
            <div className="text-right">
              <p className="text-white font-semibold tabular-nums">${price.price_usd.toFixed(4)}</p>
              <p
                className={`text-xs font-medium ${
                  price.change_24h >= 0 ? 'text-atom-green' : 'text-red-400'
                }`}
              >
                {price.change_24h >= 0 ? '+' : ''}
                {price.change_24h.toFixed(2)}%
              </p>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function ActiveSolvers() {
  const solvers = useStore((state) => Array.from(state.solvers.values()));
  const activeSolvers = solvers.filter((s) => s.status === 'active');

  const getSolverIcon = (type: string) => {
    switch (type) {
      case 'dex_router':
        return '🔀';
      case 'intent_matcher':
        return '🎯';
      case 'cex_backstop':
        return '🏦';
      default:
        return '⚡';
    }
  };

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-6">
        <h3 className="text-lg font-semibold text-white">Active Solvers</h3>
        <span className="badge-success">{activeSolvers.length} online</span>
      </div>
      <div className="space-y-3">
        {activeSolvers.map((solver) => (
          <div
            key={solver.id}
            className="flex items-center justify-between p-3 rounded-xl bg-space-900/40 border border-white/5"
          >
            <div className="flex items-center gap-3">
              <div className="w-8 h-8 rounded-lg bg-space-800 flex items-center justify-center text-lg">
                {getSolverIcon(solver.solver_type)}
              </div>
              <div>
                <p className="text-white font-medium text-sm">{solver.name}</p>
                <p className="text-xs text-space-500 capitalize">
                  {solver.solver_type.replace('_', ' ')}
                </p>
              </div>
            </div>
            <div className="text-right">
              <div className="flex items-center gap-1">
                <div className="h-1.5 w-16 rounded-full bg-space-800 overflow-hidden">
                  <div
                    className="h-full bg-gradient-to-r from-atom-green to-emerald-400 rounded-full"
                    style={{ width: `${solver.reputation_score * 100}%` }}
                  />
                </div>
                <span className="text-xs text-space-400 w-8">
                  {(solver.reputation_score * 100).toFixed(0)}%
                </span>
              </div>
              <p className="text-xs text-space-500 mt-1">reputation</p>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export default function Dashboard() {
  const stats = useStore((state) => state.stats);

  return (
    <div className="space-y-6 animate-slide-in">
      {/* Page header */}
      <div className="mb-8">
        <h2 className="text-2xl font-bold text-white tracking-tight">Dashboard</h2>
        <p className="text-space-400 mt-1">Real-time overview of the ATOM Intents system</p>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        <StatCard
          icon={Activity}
          label="Total Intents"
          value={stats?.total_intents ?? 0}
          subValue={`${stats?.pending_intents ?? 0} pending`}
          gradient="from-cosmos-600 to-cosmos-700"
        />
        <StatCard
          icon={TrendingUp}
          label="Total Auctions"
          value={stats?.total_auctions ?? 0}
          gradient="from-atom-blue to-blue-600"
          trend="up"
        />
        <StatCard
          icon={CheckCircle}
          label="Success Rate"
          value={`${((stats?.success_rate ?? 0) * 100).toFixed(1)}%`}
          gradient="from-atom-green to-emerald-600"
          trend="up"
        />
        <StatCard
          icon={Clock}
          label="Avg Execution"
          value={`${stats?.avg_execution_time_ms ?? 0}ms`}
          subValue="Target: <5000ms"
          gradient="from-atom-gold to-orange-500"
        />
        <StatCard
          icon={Users}
          label="Active Solvers"
          value={stats?.active_solvers ?? 0}
          gradient="from-purple-500 to-purple-600"
        />
        <StatCard
          icon={DollarSign}
          label="Price Improvement"
          value={`${(stats?.avg_price_improvement_bps ?? 0) / 100}%`}
          subValue="vs worst quote"
          gradient="from-teal-500 to-teal-600"
          trend="up"
        />
      </div>

      {/* Activity Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div className="lg:col-span-2">
          <RecentActivity />
        </div>
        <div className="space-y-6">
          <PriceTicker />
          <ActiveSolvers />
        </div>
      </div>
    </div>
  );
}
