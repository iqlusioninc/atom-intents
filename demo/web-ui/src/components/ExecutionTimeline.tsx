import { useMemo } from 'react';
import { useStore } from '../hooks/useStore';
import { formatDistanceToNow } from 'date-fns';
import {
  Send,
  Radio,
  Gavel,
  Trophy,
  Lock,
  ArrowRightLeft,
  CheckCircle,
  Clock,
} from 'lucide-react';

interface TimelineStep {
  id: string;
  label: string;
  description: string;
  timestamp?: string;
  duration?: number;
  status: 'completed' | 'active' | 'pending';
  icon: React.ElementType;
}

export default function ExecutionTimeline() {
  const currentAuction = useStore((state) => {
    const id = state.currentAuctionId;
    return id ? state.auctions.get(id) : null;
  });
  const quotes = useStore((state) => state.quotes);
  const settlements = useStore((state) => Array.from(state.settlements.values()));

  const latestSettlement = useMemo(() => {
    if (!currentAuction) return null;
    return settlements.find((s) => s.auction_id === currentAuction.id);
  }, [currentAuction, settlements]);

  const timeline = useMemo((): TimelineStep[] => {
    const steps: TimelineStep[] = [];

    if (!currentAuction) {
      return [
        {
          id: 'waiting',
          label: 'Waiting for Intent',
          description: 'Submit an intent to start',
          status: 'pending',
          icon: Clock,
        },
      ];
    }

    // Step 1: Intent Received
    steps.push({
      id: 'received',
      label: 'Intent Received',
      description: `${currentAuction.stats.num_intents} intent(s) in batch`,
      timestamp: currentAuction.started_at,
      status: 'completed',
      icon: Send,
    });

    // Step 2: Broadcast to Solvers
    steps.push({
      id: 'broadcast',
      label: 'Broadcast to Solvers',
      description: 'Intent sent to all active solvers',
      duration: 50,
      status: 'completed',
      icon: Radio,
    });

    // Step 3: Quote Collection
    steps.push({
      id: 'quotes',
      label: 'Quote Collection',
      description: `${quotes.length} quotes received`,
      duration: 400,
      status: quotes.length > 0 ? 'completed' : 'active',
      icon: Gavel,
    });

    // Step 4: Auction Clearing
    if (currentAuction.status === 'completed' || currentAuction.status === 'clearing') {
      steps.push({
        id: 'clearing',
        label: 'Auction Clearing',
        description: currentAuction.winning_quote
          ? `Winner: ${currentAuction.winning_quote.solver_name}`
          : 'Determining best price...',
        duration: 50,
        status: currentAuction.winning_quote ? 'completed' : 'active',
        icon: Trophy,
      });
    } else {
      steps.push({
        id: 'clearing',
        label: 'Auction Clearing',
        description: 'Waiting for quotes...',
        status: 'pending',
        icon: Trophy,
      });
    }

    // Step 5: Escrow Lock
    if (latestSettlement) {
      const hasEscrow = latestSettlement.phase !== 'init';
      steps.push({
        id: 'escrow',
        label: 'Escrow Lock',
        description: hasEscrow
          ? `Funds locked: ${latestSettlement.escrow_txid?.slice(0, 12)}...`
          : 'Locking user funds...',
        duration: 500,
        status: hasEscrow ? 'completed' : 'active',
        icon: Lock,
      });
    } else if (currentAuction.status === 'completed') {
      steps.push({
        id: 'escrow',
        label: 'Escrow Lock',
        description: 'Preparing settlement...',
        status: 'pending',
        icon: Lock,
      });
    }

    // Step 6: IBC Transfer
    if (latestSettlement && latestSettlement.phase !== 'init') {
      const hasIbc =
        latestSettlement.phase === 'ibc_in_flight' ||
        latestSettlement.phase === 'finalized';
      steps.push({
        id: 'ibc',
        label: 'IBC Transfer',
        description: hasIbc
          ? `Packet: ${latestSettlement.ibc_packet_id || 'Processing...'}`
          : 'Initiating cross-chain transfer...',
        duration: 2000,
        status: hasIbc ? 'completed' : 'active',
        icon: ArrowRightLeft,
      });
    }

    // Step 7: Settlement Complete
    if (latestSettlement?.status === 'completed') {
      steps.push({
        id: 'complete',
        label: 'Settlement Complete',
        description: `User received ${(latestSettlement.output_amount / 1_000_000).toFixed(4)} tokens`,
        timestamp: latestSettlement.completed_at || undefined,
        status: 'completed',
        icon: CheckCircle,
      });
    } else if (latestSettlement) {
      steps.push({
        id: 'complete',
        label: 'Settlement Complete',
        description: 'Finalizing...',
        status: 'pending',
        icon: CheckCircle,
      });
    }

    return steps;
  }, [currentAuction, quotes, latestSettlement]);

  // Calculate total time
  const totalTime = useMemo(() => {
    if (!currentAuction) return null;
    if (!latestSettlement?.completed_at) return null;

    const start = new Date(currentAuction.started_at);
    const end = new Date(latestSettlement.completed_at);
    return end.getTime() - start.getTime();
  }, [currentAuction, latestSettlement]);

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-6">
        <h3 className="text-lg font-semibold text-white">Execution Timeline</h3>
        {totalTime && (
          <span className="text-sm text-green-400">
            Total: {(totalTime / 1000).toFixed(1)}s
          </span>
        )}
      </div>

      <div className="relative">
        {/* Timeline line */}
        <div className="absolute left-5 top-0 bottom-0 w-0.5 bg-gray-700" />

        {/* Timeline steps */}
        <div className="space-y-6">
          {timeline.map((step, i) => {
            const Icon = step.icon;
            const isCompleted = step.status === 'completed';
            const isActive = step.status === 'active';

            return (
              <div key={step.id} className="relative flex items-start gap-4 pl-2">
                {/* Icon */}
                <div
                  className={`relative z-10 w-6 h-6 rounded-full flex items-center justify-center ${
                    isCompleted
                      ? 'bg-green-600'
                      : isActive
                      ? 'bg-cosmos-600 animate-pulse'
                      : 'bg-gray-700'
                  }`}
                >
                  <Icon className="w-3 h-3 text-white" />
                </div>

                {/* Content */}
                <div className="flex-1 pb-2">
                  <div className="flex items-center justify-between">
                    <p
                      className={`font-medium ${
                        isCompleted
                          ? 'text-green-400'
                          : isActive
                          ? 'text-cosmos-400'
                          : 'text-gray-500'
                      }`}
                    >
                      {step.label}
                    </p>
                    {step.duration && isCompleted && (
                      <span className="text-xs text-gray-500">
                        ~{step.duration}ms
                      </span>
                    )}
                  </div>
                  <p className="text-sm text-gray-400 mt-0.5">{step.description}</p>
                  {step.timestamp && (
                    <p className="text-xs text-gray-500 mt-1">
                      {formatDistanceToNow(new Date(step.timestamp), {
                        addSuffix: true,
                      })}
                    </p>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Performance summary */}
      {totalTime && totalTime < 5000 && (
        <div className="mt-4 p-3 bg-green-900/20 rounded-lg border border-green-700/50">
          <p className="text-sm text-green-300">
            Execution completed in {(totalTime / 1000).toFixed(1)}s -
            {totalTime < 3000 ? ' Excellent! ' : ' Good! '}
            Target is 2-5 seconds.
          </p>
        </div>
      )}
    </div>
  );
}
