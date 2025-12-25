import { useMemo } from 'react';
import { useStore } from '../hooks/useStore';
import { TrendingUp, TrendingDown, Minus, ExternalLink } from 'lucide-react';

interface ComparisonRow {
  venue: string;
  type: string;
  outputAmount: number;
  priceImpact: number;
  gas: number;
  netOutput: number;
  improvement: number;
}

export default function PriceComparison() {
  const prices = useStore((state) => state.prices);
  const currentAuction = useStore((state) => {
    const id = state.currentAuctionId;
    return id ? state.auctions.get(id) : null;
  });
  const quotes = useStore((state) => state.quotes);

  // Mock reference prices for comparison
  const mockComparisons = useMemo(() => {
    if (!currentAuction || quotes.length === 0) return [];

    const bestQuote = quotes.reduce((best, q) =>
      q.output_amount > best.output_amount ? q : best
    , quotes[0]);

    const inputAmount = bestQuote.input_amount / 1_000_000;
    const baseOutput = bestQuote.output_amount / 1_000_000;

    const comparisons: ComparisonRow[] = [
      {
        venue: 'ATOM Intents (Best)',
        type: 'Intent System',
        outputAmount: baseOutput,
        priceImpact: 0.15,
        gas: 0.02,
        netOutput: baseOutput - 0.02,
        improvement: 0,
      },
      {
        venue: 'Osmosis AMM',
        type: 'Direct DEX',
        outputAmount: baseOutput * 0.997,
        priceImpact: 0.30,
        gas: 0.05,
        netOutput: baseOutput * 0.997 - 0.05,
        improvement: -0.35,
      },
      {
        venue: 'Osmosis + IBC',
        type: 'Cross-chain DEX',
        outputAmount: baseOutput * 0.995,
        priceImpact: 0.45,
        gas: 0.12,
        netOutput: baseOutput * 0.995 - 0.12,
        improvement: -0.62,
      },
      {
        venue: 'Astroport (Neutron)',
        type: 'Alternative DEX',
        outputAmount: baseOutput * 0.994,
        priceImpact: 0.55,
        gas: 0.08,
        netOutput: baseOutput * 0.994 - 0.08,
        improvement: -0.72,
      },
      {
        venue: 'Aggregator (Skip)',
        type: 'DEX Aggregator',
        outputAmount: baseOutput * 0.998,
        priceImpact: 0.22,
        gas: 0.06,
        netOutput: baseOutput * 0.998 - 0.06,
        improvement: -0.28,
      },
    ];

    return comparisons;
  }, [currentAuction, quotes]);

  if (!currentAuction || quotes.length === 0) {
    return (
      <div className="card">
        <h3 className="text-lg font-semibold text-white mb-4">Price Comparison</h3>
        <p className="text-gray-400 text-center py-8">
          Submit an intent to see price comparisons across venues
        </p>
      </div>
    );
  }

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-lg font-semibold text-white">Price Comparison</h3>
        <span className="text-xs text-gray-400">vs Alternative Venues</span>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-gray-400 border-b border-gray-700">
              <th className="pb-2">Venue</th>
              <th className="pb-2 text-right">Output</th>
              <th className="pb-2 text-right">Impact</th>
              <th className="pb-2 text-right">Gas</th>
              <th className="pb-2 text-right">Net</th>
              <th className="pb-2 text-right">vs Best</th>
            </tr>
          </thead>
          <tbody>
            {mockComparisons.map((row, i) => (
              <tr
                key={row.venue}
                className={`border-b border-gray-800 ${
                  i === 0 ? 'bg-green-900/20' : ''
                }`}
              >
                <td className="py-3">
                  <div>
                    <p className={`font-medium ${i === 0 ? 'text-green-400' : 'text-white'}`}>
                      {row.venue}
                    </p>
                    <p className="text-xs text-gray-500">{row.type}</p>
                  </div>
                </td>
                <td className="py-3 text-right text-white">
                  {row.outputAmount.toFixed(4)}
                </td>
                <td className="py-3 text-right text-yellow-400">
                  -{row.priceImpact.toFixed(2)}%
                </td>
                <td className="py-3 text-right text-gray-400">
                  ${row.gas.toFixed(2)}
                </td>
                <td className="py-3 text-right text-white font-medium">
                  {row.netOutput.toFixed(4)}
                </td>
                <td className="py-3 text-right">
                  {row.improvement === 0 ? (
                    <span className="text-green-400 flex items-center justify-end gap-1">
                      <TrendingUp className="w-3 h-3" />
                      Best
                    </span>
                  ) : (
                    <span className="text-red-400 flex items-center justify-end gap-1">
                      <TrendingDown className="w-3 h-3" />
                      {row.improvement.toFixed(2)}%
                    </span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="mt-4 p-3 bg-cosmos-900/20 rounded-lg border border-cosmos-700/50">
        <p className="text-sm text-cosmos-300">
          <TrendingUp className="w-4 h-4 inline mr-1" />
          ATOM Intents provides {Math.abs(mockComparisons[1]?.improvement || 0).toFixed(2)}% better
          execution than direct DEX routing through batch auctions and solver competition.
        </p>
      </div>
    </div>
  );
}
