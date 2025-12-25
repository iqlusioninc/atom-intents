import { useMemo } from 'react';
import { formatDistanceToNow } from 'date-fns';
import { Trophy, Timer, BarChart2, Zap } from 'lucide-react';
import { useStore } from '../hooks/useStore';
import type { SolverQuote, Auction } from '../types';

function QuoteCard({ quote, isWinner }: { quote: SolverQuote; isWinner: boolean }) {
  const solverTypeLabel = {
    dex_router: { label: 'DEX Router', color: 'bg-blue-500' },
    intent_matcher: { label: 'Intent Matcher', color: 'bg-green-500' },
    cex_backstop: { label: 'CEX Backstop', color: 'bg-purple-500' },
    hybrid: { label: 'Hybrid', color: 'bg-orange-500' },
  };

  const { label, color } = solverTypeLabel[quote.solver_type] || { label: 'Unknown', color: 'bg-gray-500' };

  return (
    <div
      className={`p-4 rounded-lg transition-all ${
        isWinner
          ? 'bg-green-900/30 border-2 border-green-500 animate-pulse-glow'
          : 'bg-gray-800/50 border border-gray-700'
      }`}
    >
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2">
          {isWinner && <Trophy className="w-5 h-5 text-yellow-400" />}
          <span className="font-medium text-white">{quote.solver_name}</span>
        </div>
        <span className={`px-2 py-1 rounded text-xs text-white ${color}`}>{label}</span>
      </div>

      <div className="grid grid-cols-2 gap-4 text-sm">
        <div>
          <p className="text-gray-400">Output Amount</p>
          <p className="text-white font-medium">
            {(quote.output_amount / 1_000_000).toFixed(4)}
          </p>
        </div>
        <div>
          <p className="text-gray-400">Effective Price</p>
          <p className="text-white font-medium">{quote.effective_price.toFixed(6)}</p>
        </div>
        <div>
          <p className="text-gray-400">Confidence</p>
          <p className="text-white font-medium">{(quote.confidence * 100).toFixed(1)}%</p>
        </div>
        <div>
          <p className="text-gray-400">Est. Gas</p>
          <p className="text-white font-medium">{quote.estimated_gas.toLocaleString()}</p>
        </div>
      </div>

      <div className="mt-3 pt-3 border-t border-gray-700">
        <p className="text-xs text-gray-400 mb-1">Execution Plan: {quote.execution_plan.plan_type}</p>
        <div className="space-y-1">
          {quote.execution_plan.steps.slice(0, 3).map((step, i) => (
            <div key={i} className="text-xs text-gray-500">
              {i + 1}. {step.description}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function AuctionCard({ auction }: { auction: Auction }) {
  const statusColors = {
    open: 'badge-warning',
    collecting: 'badge-info',
    clearing: 'badge-info',
    completed: 'badge-success',
    failed: 'badge-error',
  };

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="font-medium text-white">{auction.id.slice(0, 20)}...</h3>
          <p className="text-xs text-gray-400">
            {formatDistanceToNow(new Date(auction.started_at), { addSuffix: true })}
          </p>
        </div>
        <span className={statusColors[auction.status]}>{auction.status}</span>
      </div>

      <div className="grid grid-cols-4 gap-4 text-sm">
        <div>
          <p className="text-gray-400">Intents</p>
          <p className="text-white font-medium">{auction.stats.num_intents}</p>
        </div>
        <div>
          <p className="text-gray-400">Quotes</p>
          <p className="text-white font-medium">{auction.stats.num_quotes}</p>
        </div>
        <div>
          <p className="text-gray-400">Matched</p>
          <p className="text-white font-medium">
            {(auction.stats.matched_volume / 1_000_000).toFixed(2)}
          </p>
        </div>
        <div>
          <p className="text-gray-400">Price Impr.</p>
          <p className="text-green-400 font-medium">
            +{(auction.stats.price_improvement_bps / 100).toFixed(2)}%
          </p>
        </div>
      </div>

      {auction.winning_quote && (
        <div className="mt-4 p-3 bg-green-900/20 rounded-lg border border-green-700/50">
          <div className="flex items-center gap-2 text-green-400 text-sm">
            <Trophy className="w-4 h-4" />
            <span>Winner: {auction.winning_quote.solver_name}</span>
          </div>
        </div>
      )}
    </div>
  );
}

export default function AuctionView() {
  const auctions = useStore((state) => Array.from(state.auctions.values()));
  const quotes = useStore((state) => state.quotes);
  const currentAuctionId = useStore((state) => state.currentAuctionId);

  const currentAuction = useMemo(
    () => auctions.find((a) => a.id === currentAuctionId),
    [auctions, currentAuctionId]
  );

  const sortedQuotes = useMemo(
    () => [...quotes].sort((a, b) => b.effective_price - a.effective_price),
    [quotes]
  );

  const recentAuctions = useMemo(
    () =>
      auctions
        .filter((a) => a.id !== currentAuctionId)
        .sort((a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime())
        .slice(0, 5),
    [auctions, currentAuctionId]
  );

  return (
    <div className="space-y-6 animate-slide-in">
      <div>
        <h2 className="text-2xl font-bold text-white">Auction View</h2>
        <p className="text-gray-400">Watch batch auctions and solver competition in real-time</p>
      </div>

      {/* Current Auction */}
      <div className="card">
        <div className="flex items-center gap-3 mb-4">
          <div className="p-2 bg-cosmos-600 rounded-lg">
            <Zap className="w-5 h-5 text-white" />
          </div>
          <div>
            <h3 className="text-lg font-semibold text-white">Current Auction</h3>
            {currentAuction ? (
              <p className="text-xs text-gray-400">ID: {currentAuction.id}</p>
            ) : (
              <p className="text-xs text-gray-400">Waiting for intents...</p>
            )}
          </div>
          {currentAuction && (
            <span
              className={`ml-auto ${
                currentAuction.status === 'open'
                  ? 'badge-warning'
                  : currentAuction.status === 'completed'
                  ? 'badge-success'
                  : 'badge-info'
              }`}
            >
              {currentAuction.status}
            </span>
          )}
        </div>

        {currentAuction ? (
          <div className="space-y-4">
            {/* Auction Stats */}
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
              <div className="p-3 bg-gray-800/50 rounded-lg">
                <div className="flex items-center gap-2 text-gray-400 text-sm mb-1">
                  <BarChart2 className="w-4 h-4" />
                  <span>Intents</span>
                </div>
                <p className="text-xl font-bold text-white">
                  {currentAuction.stats.num_intents}
                </p>
              </div>
              <div className="p-3 bg-gray-800/50 rounded-lg">
                <div className="flex items-center gap-2 text-gray-400 text-sm mb-1">
                  <Timer className="w-4 h-4" />
                  <span>Quotes</span>
                </div>
                <p className="text-xl font-bold text-white">{sortedQuotes.length}</p>
              </div>
              <div className="p-3 bg-gray-800/50 rounded-lg">
                <p className="text-gray-400 text-sm mb-1">Competition Score</p>
                <p className="text-xl font-bold text-white">
                  {(currentAuction.stats.solver_competition_score * 100).toFixed(0)}%
                </p>
              </div>
              <div className="p-3 bg-gray-800/50 rounded-lg">
                <p className="text-gray-400 text-sm mb-1">Clearing Price</p>
                <p className="text-xl font-bold text-white">
                  {currentAuction.clearing_price?.toFixed(6) ?? '—'}
                </p>
              </div>
            </div>

            {/* Quotes */}
            {sortedQuotes.length > 0 && (
              <div>
                <h4 className="text-sm font-medium text-gray-400 mb-3">
                  Solver Quotes (sorted by price)
                </h4>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {sortedQuotes.map((quote, i) => (
                    <QuoteCard
                      key={quote.id}
                      quote={quote}
                      isWinner={i === 0 && currentAuction.status === 'completed'}
                    />
                  ))}
                </div>
              </div>
            )}
          </div>
        ) : (
          <div className="text-center py-12">
            <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-gray-800 flex items-center justify-center">
              <Timer className="w-8 h-8 text-gray-600" />
            </div>
            <p className="text-gray-400">No active auction</p>
            <p className="text-sm text-gray-500 mt-1">
              Submit an intent to start an auction
            </p>
          </div>
        )}
      </div>

      {/* Recent Auctions */}
      {recentAuctions.length > 0 && (
        <div>
          <h3 className="text-lg font-semibold text-white mb-4">Recent Auctions</h3>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {recentAuctions.map((auction) => (
              <AuctionCard key={auction.id} auction={auction} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
