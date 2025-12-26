import { Activity, TrendingUp, Clock, CheckCircle, Users, DollarSign } from 'lucide-react';
import { useStore } from '../hooks/useStore';
import { formatDistanceToNow } from 'date-fns';

function StatCard({
  icon: Icon,
  label,
  value,
  subValue,
  color,
}: {
  icon: React.ElementType;
  label: string;
  value: string | number;
  subValue?: string;
  color: string;
}) {
  return (
    <div className="card !p-3 sm:!p-6">
      <div className="flex items-center gap-3 sm:gap-4">
        <div className={`p-2 sm:p-3 rounded-lg ${color} flex-shrink-0`}>
          <Icon className="w-5 h-5 sm:w-6 sm:h-6 text-white" />
        </div>
        <div className="min-w-0">
          <p className="text-gray-400 text-xs sm:text-sm">{label}</p>
          <p className="text-lg sm:text-2xl font-bold text-white truncate">{value}</p>
          {subValue && <p className="text-xs text-gray-500 truncate">{subValue}</p>}
        </div>
      </div>
    </div>
  );
}

function RecentActivity() {
  const intents = useStore((state) => Array.from(state.intents.values()));
  const recentIntents = intents
    .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
    .slice(0, 5);

  const getStatusColor = (status: string) => {
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

  return (
    <div className="card !p-3 sm:!p-6">
      <h3 className="text-base sm:text-lg font-semibold text-white mb-3 sm:mb-4">Recent Activity</h3>
      <div className="space-y-2 sm:space-y-3">
        {recentIntents.length === 0 ? (
          <p className="text-gray-500 text-center py-4 text-sm">No recent activity</p>
        ) : (
          recentIntents.map((intent) => (
            <div
              key={intent.id}
              className="flex items-center justify-between p-2 sm:p-3 bg-gray-800/50 rounded-lg gap-2"
            >
              <div className="flex items-center gap-2 sm:gap-3 min-w-0">
                <div className="text-xl sm:text-2xl flex-shrink-0">
                  {intent.input.denom === 'ATOM' ? '⚛️' : intent.input.denom === 'OSMO' ? '🧪' : '💵'}
                </div>
                <div className="min-w-0">
                  <p className="text-white font-medium text-sm sm:text-base truncate">
                    {(intent.input.amount / 1_000_000).toFixed(2)} {intent.input.denom} → {intent.output.denom}
                  </p>
                  <p className="text-xs text-gray-400">
                    {formatDistanceToNow(new Date(intent.created_at), { addSuffix: true })}
                  </p>
                </div>
              </div>
              <span className={`${getStatusColor(intent.status)} flex-shrink-0 text-[10px] sm:text-xs`}>{intent.status}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function PriceTicker() {
  const prices = useStore((state) => Array.from(state.prices.values()));

  return (
    <div className="card !p-3 sm:!p-6">
      <h3 className="text-base sm:text-lg font-semibold text-white mb-3 sm:mb-4">Live Prices</h3>
      <div className="space-y-2 sm:space-y-3">
        {prices.map((price) => (
          <div
            key={price.denom}
            className="flex items-center justify-between p-2 sm:p-3 bg-gray-800/50 rounded-lg"
          >
            <div className="flex items-center gap-2 sm:gap-3">
              <span className="text-lg sm:text-xl">
                {price.denom === 'ATOM' ? '⚛️' : price.denom === 'OSMO' ? '🧪' : price.denom === 'NTRN' ? '⚡' : '💵'}
              </span>
              <span className="text-white font-medium text-sm sm:text-base">{price.denom}</span>
            </div>
            <div className="text-right">
              <p className="text-white font-medium text-sm sm:text-base">${price.price_usd.toFixed(4)}</p>
              <p
                className={`text-xs ${
                  price.change_24h >= 0 ? 'text-green-400' : 'text-red-400'
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

  return (
    <div className="card !p-3 sm:!p-6">
      <h3 className="text-base sm:text-lg font-semibold text-white mb-3 sm:mb-4">Active Solvers</h3>
      <div className="space-y-2 sm:space-y-3">
        {activeSolvers.map((solver) => (
          <div
            key={solver.id}
            className="flex items-center justify-between p-2 sm:p-3 bg-gray-800/50 rounded-lg"
          >
            <div className="flex items-center gap-2 sm:gap-3 min-w-0">
              <span className="status-dot-active flex-shrink-0" />
              <div className="min-w-0">
                <p className="text-white font-medium text-sm sm:text-base truncate">{solver.name}</p>
                <p className="text-xs text-gray-400 capitalize truncate">{solver.solver_type.replace('_', ' ')}</p>
              </div>
            </div>
            <div className="text-right flex-shrink-0">
              <p className="text-white text-sm">{(solver.reputation_score * 100).toFixed(0)}%</p>
              <p className="text-xs text-gray-400">reputation</p>
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
    <div className="space-y-4 sm:space-y-6 animate-slide-in">
      <div>
        <h2 className="text-xl sm:text-2xl font-bold text-white">Dashboard</h2>
        <p className="text-gray-400 text-sm sm:text-base">Real-time overview of the ATOM Intents system</p>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-2 sm:grid-cols-2 lg:grid-cols-3 gap-2 sm:gap-4">
        <StatCard
          icon={Activity}
          label="Total Intents"
          value={stats?.total_intents ?? 0}
          subValue={`${stats?.pending_intents ?? 0} pending`}
          color="bg-cosmos-600"
        />
        <StatCard
          icon={TrendingUp}
          label="Total Auctions"
          value={stats?.total_auctions ?? 0}
          color="bg-blue-600"
        />
        <StatCard
          icon={CheckCircle}
          label="Success Rate"
          value={`${((stats?.success_rate ?? 0) * 100).toFixed(1)}%`}
          color="bg-green-600"
        />
        <StatCard
          icon={Clock}
          label="Avg Execution"
          value={`${stats?.avg_execution_time_ms ?? 0}ms`}
          color="bg-yellow-600"
        />
        <StatCard
          icon={Users}
          label="Active Solvers"
          value={stats?.active_solvers ?? 0}
          color="bg-purple-600"
        />
        <StatCard
          icon={DollarSign}
          label="Price Improvement"
          value={`${(stats?.avg_price_improvement_bps ?? 0) / 100}%`}
          subValue="vs worst quote"
          color="bg-emerald-600"
        />
      </div>

      {/* Activity Grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 sm:gap-6">
        <div className="lg:col-span-2">
          <RecentActivity />
        </div>
        <div className="space-y-4 sm:space-y-6">
          <PriceTicker />
          <ActiveSolvers />
        </div>
      </div>
    </div>
  );
}
