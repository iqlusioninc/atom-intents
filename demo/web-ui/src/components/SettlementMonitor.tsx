import { formatDistanceToNow } from 'date-fns';
import { useStore } from '../hooks/useStore';
import {
  CheckCircle,
  Clock,
  XCircle,
  ArrowRight,
  Lock,
  Unlock,
  Send,
  RefreshCw,
} from 'lucide-react';
import type { Settlement, SettlementPhase } from '../types';

function PhaseIndicator({ phase, status }: { phase: SettlementPhase; status: string }) {
  const phases: Array<{
    key: SettlementPhase;
    label: string;
    icon: React.ElementType;
  }> = [
    { key: 'init', label: 'Initialize', icon: Clock },
    { key: 'escrow_locked', label: 'Escrow Locked', icon: Lock },
    { key: 'solver_committed', label: 'Solver Committed', icon: CheckCircle },
    { key: 'ibc_in_flight', label: 'IBC Transfer', icon: Send },
    { key: 'finalized', label: 'Finalized', icon: Unlock },
  ];

  const currentIndex = phases.findIndex((p) => p.key === phase);
  const isFailed = status === 'failed' || status === 'refunded';

  return (
    <div className="flex items-center gap-2">
      {phases.map((p, i) => {
        const Icon = p.icon;
        const isComplete = i < currentIndex || (i === currentIndex && phase === 'finalized');
        const isCurrent = i === currentIndex;
        const isError = isCurrent && isFailed;

        return (
          <div key={p.key} className="flex items-center">
            <div
              className={`flex items-center gap-2 px-3 py-1 rounded-full text-xs ${
                isError
                  ? 'bg-red-900/50 text-red-400'
                  : isComplete
                  ? 'bg-green-900/50 text-green-400'
                  : isCurrent
                  ? 'bg-cosmos-900/50 text-cosmos-400'
                  : 'bg-gray-800 text-gray-500'
              }`}
            >
              <Icon className="w-3 h-3" />
              <span>{p.label}</span>
            </div>
            {i < phases.length - 1 && (
              <ArrowRight
                className={`w-4 h-4 mx-1 ${
                  i < currentIndex ? 'text-green-500' : 'text-gray-600'
                }`}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function SettlementCard({ settlement }: { settlement: Settlement }) {
  const statusConfig = {
    pending: { label: 'Pending', class: 'badge-warning', icon: Clock },
    committing: { label: 'Committing', class: 'badge-info', icon: Lock },
    executing: { label: 'Executing', class: 'badge-info', icon: Send },
    completed: { label: 'Completed', class: 'badge-success', icon: CheckCircle },
    failed: { label: 'Failed', class: 'badge-error', icon: XCircle },
    refunded: { label: 'Refunded', class: 'badge-warning', icon: RefreshCw },
  };

  const { label, class: badgeClass, icon: StatusIcon } = statusConfig[settlement.status];

  return (
    <div className="card">
      <div className="flex items-start justify-between mb-4">
        <div>
          <h3 className="font-medium text-white">
            {settlement.id.slice(0, 24)}...
          </h3>
          <p className="text-xs text-gray-400">
            Created {formatDistanceToNow(new Date(settlement.created_at), { addSuffix: true })}
          </p>
        </div>
        <span className={`${badgeClass} flex items-center gap-1`}>
          <StatusIcon className="w-3 h-3" />
          {label}
        </span>
      </div>

      {/* Phase Progress */}
      <div className="mb-4 overflow-x-auto pb-2">
        <PhaseIndicator phase={settlement.phase} status={settlement.status} />
      </div>

      {/* Settlement Details */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm mb-4">
        <div>
          <p className="text-gray-400">Input Amount</p>
          <p className="text-white font-medium">
            {(settlement.input_amount / 1_000_000).toFixed(4)}
          </p>
        </div>
        <div>
          <p className="text-gray-400">Output Amount</p>
          <p className="text-white font-medium">
            {(settlement.output_amount / 1_000_000).toFixed(4)}
          </p>
        </div>
        <div>
          <p className="text-gray-400">Solver</p>
          <p className="text-white font-medium truncate">{settlement.solver_id.slice(0, 16)}...</p>
        </div>
        <div>
          <p className="text-gray-400">Intents</p>
          <p className="text-white font-medium">{settlement.intent_ids.length}</p>
        </div>
      </div>

      {/* Transaction IDs */}
      {(settlement.escrow_txid || settlement.ibc_packet_id || settlement.execution_txid) && (
        <div className="pt-4 border-t border-gray-700 space-y-2">
          {settlement.escrow_txid && (
            <div className="flex items-center justify-between text-xs">
              <span className="text-gray-400">Escrow TX</span>
              <code className="text-cosmos-400 font-mono">{settlement.escrow_txid.slice(0, 20)}...</code>
            </div>
          )}
          {settlement.ibc_packet_id && (
            <div className="flex items-center justify-between text-xs">
              <span className="text-gray-400">IBC Packet</span>
              <code className="text-blue-400 font-mono">{settlement.ibc_packet_id}</code>
            </div>
          )}
          {settlement.execution_txid && (
            <div className="flex items-center justify-between text-xs">
              <span className="text-gray-400">Execution TX</span>
              <code className="text-green-400 font-mono">{settlement.execution_txid.slice(0, 20)}...</code>
            </div>
          )}
        </div>
      )}

      {/* Event Timeline */}
      {settlement.events.length > 0 && (
        <div className="mt-4 pt-4 border-t border-gray-700">
          <p className="text-xs text-gray-400 mb-2">Event Timeline</p>
          <div className="space-y-2 max-h-32 overflow-y-auto">
            {settlement.events.map((event, i) => (
              <div
                key={i}
                className="flex items-start gap-2 text-xs"
              >
                <div className="w-1.5 h-1.5 rounded-full bg-cosmos-500 mt-1.5" />
                <div>
                  <span className="text-gray-300">{event.description}</span>
                  <span className="text-gray-500 ml-2">
                    {formatDistanceToNow(new Date(event.timestamp), { addSuffix: true })}
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export default function SettlementMonitor() {
  const settlements = useStore((state) => Array.from(state.settlements.values()));

  const sortedSettlements = settlements.sort(
    (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
  );

  const activeSettlements = settlements.filter(
    (s) => s.status !== 'completed' && s.status !== 'failed' && s.status !== 'refunded'
  );
  const completedCount = settlements.filter((s) => s.status === 'completed').length;
  const failedCount = settlements.filter(
    (s) => s.status === 'failed' || s.status === 'refunded'
  ).length;

  return (
    <div className="space-y-6 animate-slide-in">
      <div>
        <h2 className="text-2xl font-bold text-white">Settlement Monitor</h2>
        <p className="text-gray-400">Track settlement lifecycle and IBC transfers</p>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <div className="card">
          <p className="text-gray-400 text-sm">Total Settlements</p>
          <p className="text-2xl font-bold text-white">{settlements.length}</p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Active</p>
          <p className="text-2xl font-bold text-yellow-400">{activeSettlements.length}</p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Completed</p>
          <p className="text-2xl font-bold text-green-400">{completedCount}</p>
        </div>
        <div className="card">
          <p className="text-gray-400 text-sm">Failed/Refunded</p>
          <p className="text-2xl font-bold text-red-400">{failedCount}</p>
        </div>
      </div>

      {/* Settlement List */}
      {sortedSettlements.length > 0 ? (
        <div className="space-y-4">
          {sortedSettlements.map((settlement) => (
            <SettlementCard key={settlement.id} settlement={settlement} />
          ))}
        </div>
      ) : (
        <div className="card text-center py-12">
          <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-gray-800 flex items-center justify-center">
            <Clock className="w-8 h-8 text-gray-600" />
          </div>
          <p className="text-gray-400">No settlements yet</p>
          <p className="text-sm text-gray-500 mt-1">
            Settlements are created when auctions complete
          </p>
        </div>
      )}

      {/* Two-Phase Commit Explainer */}
      <div className="card bg-cosmos-900/20 border-cosmos-700/50">
        <h3 className="text-lg font-semibold text-white mb-4">Two-Phase Settlement Protocol</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <div>
            <h4 className="font-medium text-white mb-2">Phase 1: Commit</h4>
            <ol className="text-sm text-gray-400 space-y-2">
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">1.</span>
                User funds are locked in the escrow contract
              </li>
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">2.</span>
                Solver commits to providing the output amount
              </li>
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">3.</span>
                Both parties are now bound to the settlement
              </li>
            </ol>
          </div>
          <div>
            <h4 className="font-medium text-white mb-2">Phase 2: Execute</h4>
            <ol className="text-sm text-gray-400 space-y-2">
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">4.</span>
                IBC packets are submitted (prioritized by relayer)
              </li>
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">5.</span>
                Cross-chain swaps execute via Wasm hooks
              </li>
              <li className="flex items-start gap-2">
                <span className="text-cosmos-400">6.</span>
                User receives funds, settlement finalized
              </li>
            </ol>
          </div>
        </div>
      </div>
    </div>
  );
}
