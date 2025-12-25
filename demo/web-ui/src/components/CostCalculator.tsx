import { useState, useMemo } from 'react';
import { useStore } from '../hooks/useStore';
import { Calculator, DollarSign, Percent, Clock, Zap } from 'lucide-react';
import { TOKENS } from '../types';

export default function CostCalculator() {
  const prices = useStore((state) => state.prices);
  const [inputDenom, setInputDenom] = useState('ATOM');
  const [inputAmount, setInputAmount] = useState('100');
  const [outputDenom, setOutputDenom] = useState('OSMO');

  const calculation = useMemo(() => {
    const input = parseFloat(inputAmount) || 0;
    const inputPrice = prices.get(inputDenom)?.price_usd || 0;
    const outputPrice = prices.get(outputDenom)?.price_usd || 0;

    const inputValueUsd = input * inputPrice;

    // Calculate for different methods
    const methods = {
      atomIntents: {
        name: 'ATOM Intents',
        spread: 0.15, // 15 bps
        gas: 0.02, // $0.02
        slippage: 0.10, // 10 bps
        time: 3, // seconds
      },
      directDex: {
        name: 'Direct DEX',
        spread: 0.30,
        gas: 0.05,
        slippage: 0.25,
        time: 15,
      },
      aggregator: {
        name: 'DEX Aggregator',
        spread: 0.22,
        gas: 0.06,
        slippage: 0.15,
        time: 20,
      },
      cex: {
        name: 'CEX (Binance)',
        spread: 0.08,
        gas: 0, // no on-chain gas
        slippage: 0.05,
        time: 1, // but requires withdrawal
        withdrawalFee: 0.5, // $0.50
        withdrawalTime: 600, // 10 minutes
      },
    };

    const results = Object.entries(methods).map(([key, method]) => {
      const totalCostBps =
        method.spread + method.slippage + (method.gas / inputValueUsd) * 10000;
      const outputAmount =
        outputPrice > 0 ? (inputValueUsd * (1 - totalCostBps / 10000)) / outputPrice : 0;
      const netCostUsd = inputValueUsd * (totalCostBps / 10000) + method.gas;

      return {
        key,
        ...method,
        totalCostBps,
        outputAmount,
        netCostUsd,
        effectiveTime: method.withdrawalTime
          ? method.time + method.withdrawalTime
          : method.time,
      };
    });

    // Sort by total cost
    results.sort((a, b) => a.totalCostBps - b.totalCostBps);

    return {
      inputValueUsd,
      outputPrice,
      results,
      savings: results[1] ? results[1].netCostUsd - results[0].netCostUsd : 0,
    };
  }, [inputAmount, inputDenom, outputDenom, prices]);

  return (
    <div className="card">
      <div className="flex items-center gap-3 mb-6">
        <div className="p-2 bg-cosmos-600 rounded-lg">
          <Calculator className="w-5 h-5 text-white" />
        </div>
        <div>
          <h3 className="text-lg font-semibold text-white">Cost Calculator</h3>
          <p className="text-xs text-gray-400">Compare trading costs across venues</p>
        </div>
      </div>

      {/* Input form */}
      <div className="grid grid-cols-3 gap-4 mb-6">
        <div>
          <label className="block text-xs text-gray-400 mb-1">Amount</label>
          <input
            type="number"
            value={inputAmount}
            onChange={(e) => setInputAmount(e.target.value)}
            className="input w-full"
            placeholder="100"
          />
        </div>
        <div>
          <label className="block text-xs text-gray-400 mb-1">From</label>
          <select
            value={inputDenom}
            onChange={(e) => setInputDenom(e.target.value)}
            className="select w-full"
          >
            {Object.entries(TOKENS).map(([denom, token]) => (
              <option key={denom} value={denom}>
                {token.logo} {denom}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-400 mb-1">To</label>
          <select
            value={outputDenom}
            onChange={(e) => setOutputDenom(e.target.value)}
            className="select w-full"
          >
            {Object.entries(TOKENS)
              .filter(([d]) => d !== inputDenom)
              .map(([denom, token]) => (
                <option key={denom} value={denom}>
                  {token.logo} {denom}
                </option>
              ))}
          </select>
        </div>
      </div>

      <div className="text-sm text-gray-400 mb-4">
        Trade value: ${calculation.inputValueUsd.toFixed(2)} USD
      </div>

      {/* Results table */}
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-gray-400 border-b border-gray-700">
              <th className="pb-2">Venue</th>
              <th className="pb-2 text-right">Cost</th>
              <th className="pb-2 text-right">Output</th>
              <th className="pb-2 text-right">Time</th>
            </tr>
          </thead>
          <tbody>
            {calculation.results.map((result, i) => (
              <tr
                key={result.key}
                className={`border-b border-gray-800 ${
                  i === 0 ? 'bg-green-900/20' : ''
                }`}
              >
                <td className="py-3">
                  <span
                    className={`font-medium ${
                      i === 0 ? 'text-green-400' : 'text-white'
                    }`}
                  >
                    {result.name}
                  </span>
                  {i === 0 && (
                    <span className="ml-2 text-xs bg-green-600 text-white px-1.5 py-0.5 rounded">
                      Best
                    </span>
                  )}
                </td>
                <td className="py-3 text-right">
                  <div className="text-white">${result.netCostUsd.toFixed(2)}</div>
                  <div className="text-xs text-gray-500">
                    {result.totalCostBps.toFixed(1)} bps
                  </div>
                </td>
                <td className="py-3 text-right text-white">
                  {result.outputAmount.toFixed(4)}
                </td>
                <td className="py-3 text-right">
                  <div className="text-white">{result.effectiveTime}s</div>
                  {result.withdrawalTime && (
                    <div className="text-xs text-gray-500">+withdrawal</div>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Savings callout */}
      {calculation.savings > 0 && (
        <div className="mt-4 p-4 bg-green-900/20 rounded-lg border border-green-700/50">
          <div className="flex items-center gap-3">
            <div className="p-2 bg-green-600 rounded-full">
              <DollarSign className="w-4 h-4 text-white" />
            </div>
            <div>
              <p className="text-green-400 font-medium">
                Save ${calculation.savings.toFixed(2)} with ATOM Intents
              </p>
              <p className="text-sm text-gray-400">
                vs the next best option
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Breakdown legend */}
      <div className="mt-4 grid grid-cols-3 gap-4 text-xs">
        <div className="flex items-center gap-2 text-gray-400">
          <Percent className="w-3 h-3" />
          <span>Spread + Slippage</span>
        </div>
        <div className="flex items-center gap-2 text-gray-400">
          <Zap className="w-3 h-3" />
          <span>Gas Fees</span>
        </div>
        <div className="flex items-center gap-2 text-gray-400">
          <Clock className="w-3 h-3" />
          <span>Settlement Time</span>
        </div>
      </div>
    </div>
  );
}
