import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowRight, Loader2, Check, Wallet, AlertCircle } from 'lucide-react';
import * as api from '../services/api';
import { useStore } from '../hooks/useStore';
import { useWallet } from '../hooks/useWallet';
import { TOKENS, CHAINS } from '../types';

export default function IntentCreator() {
  const prices = useStore((state) => state.prices);
  const addIntent = useStore((state) => state.addIntent);
  const { connected, address, balances, updateBalance } = useWallet();

  const [inputDenom, setInputDenom] = useState('ATOM');
  const [outputDenom, setOutputDenom] = useState('OSMO');
  const [inputAmount, setInputAmount] = useState('10');
  const [slippage, setSlippage] = useState('1');
  const [success, setSuccess] = useState(false);

  const inputPrice = prices.get(inputDenom)?.price_usd ?? 0;
  const outputPrice = prices.get(outputDenom)?.price_usd ?? 0;

  const inputValueUsd = parseFloat(inputAmount || '0') * inputPrice;
  const estimatedOutput = outputPrice > 0 ? inputValueUsd / outputPrice : 0;
  const minOutput = estimatedOutput * (1 - parseFloat(slippage) / 100);

  // Get user's balance for the input token
  const userBalance = balances[inputDenom] || 0;
  const userBalanceFormatted = userBalance / 1_000_000;
  const inputAmountMicro = Math.floor(parseFloat(inputAmount || '0') * 1_000_000);
  const hasInsufficientBalance = inputAmountMicro > userBalance;

  const mutation = useMutation({
    mutationFn: api.submitIntent,
    onSuccess: (data) => {
      addIntent(data.intent);
      setSuccess(true);
      // Deduct from simulated balance
      if (connected) {
        updateBalance(inputDenom, userBalance - inputAmountMicro);
      }
      setTimeout(() => setSuccess(false), 3000);
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    if (!connected) {
      return;
    }

    const inputToken = TOKENS[inputDenom];
    const outputToken = TOKENS[outputDenom];

    mutation.mutate({
      user_address: address || `cosmos1demo${Math.random().toString(16).slice(2, 10)}`,
      input: {
        chain_id: inputToken.chain,
        denom: inputDenom,
        amount: Math.floor(parseFloat(inputAmount) * 1_000_000),
      },
      output: {
        chain_id: outputToken.chain,
        denom: outputDenom,
        min_amount: Math.floor(minOutput * 1_000_000),
      },
      fill_config: {
        allow_partial: true,
        min_fill_percent: 80,
        strategy: 'eager',
      },
      constraints: {
        max_hops: 3,
        allowed_venues: [],
        excluded_venues: [],
        max_slippage_bps: Math.floor(parseFloat(slippage) * 100),
      },
      timeout_seconds: 60,
    });
  };

  return (
    <div className="space-y-6 animate-slide-in">
      <div>
        <h2 className="text-2xl font-bold text-white">Create Intent</h2>
        <p className="text-gray-400">Submit a new trading intent to the system</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Intent Form */}
        <form onSubmit={handleSubmit} className="card space-y-6">
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              From (Input)
            </label>
            <div className="flex gap-3">
              <select
                value={inputDenom}
                onChange={(e) => setInputDenom(e.target.value)}
                className="select flex-1"
              >
                {Object.entries(TOKENS).map(([denom, token]) => (
                  <option key={denom} value={denom}>
                    {token.logo} {denom}
                  </option>
                ))}
              </select>
              <input
                type="number"
                value={inputAmount}
                onChange={(e) => setInputAmount(e.target.value)}
                placeholder="Amount"
                className="input flex-1"
                step="0.01"
                min="0"
              />
            </div>
            <div className="flex items-center justify-between mt-1">
              <p className="text-xs text-gray-500">
                ≈ ${inputValueUsd.toFixed(2)} USD @ ${inputPrice.toFixed(4)}/{inputDenom}
              </p>
              {connected && (
                <p className="text-xs text-gray-400">
                  Balance: <span className={hasInsufficientBalance ? 'text-red-400' : 'text-green-400'}>
                    {userBalanceFormatted.toFixed(2)} {inputDenom}
                  </span>
                </p>
              )}
            </div>
            {hasInsufficientBalance && connected && (
              <p className="text-xs text-red-400 mt-1 flex items-center gap-1">
                <AlertCircle className="w-3 h-3" />
                Insufficient balance
              </p>
            )}
          </div>

          <div className="flex justify-center">
            <div className="p-2 bg-gray-800 rounded-full">
              <ArrowRight className="w-5 h-5 text-cosmos-400" />
            </div>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              To (Output)
            </label>
            <div className="flex gap-3">
              <select
                value={outputDenom}
                onChange={(e) => setOutputDenom(e.target.value)}
                className="select flex-1"
              >
                {Object.entries(TOKENS)
                  .filter(([denom]) => denom !== inputDenom)
                  .map(([denom, token]) => (
                    <option key={denom} value={denom}>
                      {token.logo} {denom}
                    </option>
                  ))}
              </select>
              <input
                type="text"
                value={estimatedOutput.toFixed(4)}
                readOnly
                className="input flex-1 bg-gray-900/50"
              />
            </div>
            <p className="text-xs text-gray-500 mt-1">
              Estimated output @ ${outputPrice.toFixed(4)}/{outputDenom}
            </p>
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              Slippage Tolerance
            </label>
            <div className="flex gap-2">
              {['0.5', '1', '2', '5'].map((val) => (
                <button
                  key={val}
                  type="button"
                  onClick={() => setSlippage(val)}
                  className={`px-4 py-2 rounded-lg transition-colors ${
                    slippage === val
                      ? 'bg-cosmos-600 text-white'
                      : 'bg-gray-800 text-gray-400 hover:bg-gray-700'
                  }`}
                >
                  {val}%
                </button>
              ))}
            </div>
            <p className="text-xs text-gray-500 mt-2">
              Minimum output: {minOutput.toFixed(4)} {outputDenom}
            </p>
          </div>

          {!connected ? (
            <div className="p-4 bg-yellow-900/20 border border-yellow-700/50 rounded-lg">
              <div className="flex items-center gap-3">
                <Wallet className="w-5 h-5 text-yellow-400" />
                <div>
                  <p className="text-yellow-300 font-medium">Connect Wallet</p>
                  <p className="text-sm text-gray-400">
                    Connect a wallet to submit intents
                  </p>
                </div>
              </div>
            </div>
          ) : (
            <button
              type="submit"
              disabled={mutation.isPending || !inputAmount || parseFloat(inputAmount) <= 0 || hasInsufficientBalance}
              className="w-full btn-primary flex items-center justify-center gap-2 py-3 disabled:opacity-50"
            >
              {mutation.isPending ? (
                <>
                  <Loader2 className="w-5 h-5 animate-spin" />
                  Submitting...
                </>
              ) : success ? (
                <>
                  <Check className="w-5 h-5" />
                  Intent Submitted!
                </>
              ) : (
                'Submit Intent'
              )}
            </button>
          )}

          {mutation.isError && (
            <p className="text-red-400 text-sm text-center">
              Error: {(mutation.error as Error).message}
            </p>
          )}
        </form>

        {/* Intent Preview */}
        <div className="card space-y-4">
          <h3 className="text-lg font-semibold text-white">Intent Preview</h3>

          <div className="space-y-4">
            <div className="p-4 bg-gray-800/50 rounded-lg">
              <p className="text-sm text-gray-400 mb-2">Trade Summary</p>
              <div className="flex items-center gap-3">
                <div className="text-center">
                  <p className="text-2xl mb-1">{TOKENS[inputDenom]?.logo}</p>
                  <p className="text-white font-medium">{inputAmount} {inputDenom}</p>
                  <p className="text-xs text-gray-400">{TOKENS[inputDenom]?.name}</p>
                </div>
                <ArrowRight className="w-6 h-6 text-cosmos-400 flex-shrink-0" />
                <div className="text-center">
                  <p className="text-2xl mb-1">{TOKENS[outputDenom]?.logo}</p>
                  <p className="text-white font-medium">{estimatedOutput.toFixed(4)} {outputDenom}</p>
                  <p className="text-xs text-gray-400">{TOKENS[outputDenom]?.name}</p>
                </div>
              </div>
            </div>

            <div className="p-4 bg-gray-800/50 rounded-lg space-y-2">
              <p className="text-sm text-gray-400">Execution Details</p>
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <p className="text-gray-500">Source Chain</p>
                  <p className="text-white">{TOKENS[inputDenom]?.chain}</p>
                </div>
                <div>
                  <p className="text-gray-500">Destination Chain</p>
                  <p className="text-white">{TOKENS[outputDenom]?.chain}</p>
                </div>
                <div>
                  <p className="text-gray-500">Max Slippage</p>
                  <p className="text-white">{slippage}%</p>
                </div>
                <div>
                  <p className="text-gray-500">Timeout</p>
                  <p className="text-white">60 seconds</p>
                </div>
              </div>
            </div>

            <div className="p-4 bg-cosmos-900/30 border border-cosmos-700/50 rounded-lg">
              <p className="text-sm text-cosmos-300 mb-2">How it works:</p>
              <ol className="text-sm text-gray-400 space-y-1">
                <li>1. Your intent is broadcast to all solvers</li>
                <li>2. Solvers compete in a batch auction</li>
                <li>3. Best price wins, funds are escrowed</li>
                <li>4. Settlement via IBC (2-5 seconds)</li>
              </ol>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
