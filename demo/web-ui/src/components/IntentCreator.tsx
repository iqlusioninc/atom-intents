import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowRight, Loader2, Check, Wallet, AlertCircle, Info } from 'lucide-react';
import * as api from '../services/api';
import { useStore } from '../hooks/useStore';
import { useWallet } from '../hooks/useWallet';
import { TOKENS } from '../types';

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

  const getTokenIcon = (denom: string) => {
    const colors: Record<string, { bg: string; text: string }> = {
      ATOM: { bg: 'bg-cosmos-500/20', text: 'text-cosmos-400' },
      OSMO: { bg: 'bg-pink-500/20', text: 'text-pink-400' },
      USDC: { bg: 'bg-blue-500/20', text: 'text-blue-400' },
      NTRN: { bg: 'bg-orange-500/20', text: 'text-orange-400' },
    };
    const style = colors[denom] || { bg: 'bg-space-700', text: 'text-space-300' };

    return (
      <div className={`w-10 h-10 rounded-full ${style.bg} flex items-center justify-center`}>
        <span className={`${style.text} text-lg font-bold`}>{denom[0]}</span>
      </div>
    );
  };

  return (
    <div className="space-y-6 animate-slide-in">
      <div className="mb-8">
        <h2 className="text-2xl font-bold text-white tracking-tight">Create Intent</h2>
        <p className="text-space-400 mt-1">Submit a new trading intent to the system</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Intent Form */}
        <form onSubmit={handleSubmit} className="card space-y-6">
          {/* Input Token */}
          <div>
            <label className="block text-sm font-medium text-space-300 mb-3">
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
                    {denom} - {token.name}
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
            <div className="flex items-center justify-between mt-2">
              <p className="text-xs text-space-500">
                ≈ ${inputValueUsd.toFixed(2)} USD @ ${inputPrice.toFixed(4)}/{inputDenom}
              </p>
              {connected && (
                <p className="text-xs text-space-400">
                  Balance:{' '}
                  <span className={hasInsufficientBalance ? 'text-red-400' : 'text-atom-green'}>
                    {userBalanceFormatted.toFixed(2)} {inputDenom}
                  </span>
                </p>
              )}
            </div>
            {hasInsufficientBalance && connected && (
              <div className="mt-2 flex items-center gap-1.5 text-xs text-red-400">
                <AlertCircle className="w-3.5 h-3.5" />
                Insufficient balance
              </div>
            )}
          </div>

          {/* Swap Arrow */}
          <div className="flex justify-center">
            <div className="p-3 rounded-xl bg-space-800/80 border border-white/5">
              <ArrowRight className="w-5 h-5 text-cosmos-400" />
            </div>
          </div>

          {/* Output Token */}
          <div>
            <label className="block text-sm font-medium text-space-300 mb-3">
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
                      {denom} - {token.name}
                    </option>
                  ))}
              </select>
              <input
                type="text"
                value={estimatedOutput.toFixed(4)}
                readOnly
                className="input flex-1 bg-space-900/50 text-space-300"
              />
            </div>
            <p className="text-xs text-space-500 mt-2">
              Estimated output @ ${outputPrice.toFixed(4)}/{outputDenom}
            </p>
          </div>

          {/* Slippage */}
          <div>
            <label className="block text-sm font-medium text-space-300 mb-3">
              Slippage Tolerance
            </label>
            <div className="flex gap-2">
              {['0.5', '1', '2', '5'].map((val) => (
                <button
                  key={val}
                  type="button"
                  onClick={() => setSlippage(val)}
                  className={`flex-1 px-4 py-2.5 rounded-xl font-medium text-sm transition-all ${
                    slippage === val
                      ? 'bg-cosmos-500/20 text-cosmos-400 border border-cosmos-500/30'
                      : 'bg-space-800/80 text-space-400 border border-white/5 hover:bg-space-700/80 hover:text-white'
                  }`}
                >
                  {val}%
                </button>
              ))}
            </div>
            <p className="text-xs text-space-500 mt-2">
              Minimum output: {minOutput.toFixed(4)} {outputDenom}
            </p>
          </div>

          {/* Submit Button or Wallet Prompt */}
          {!connected ? (
            <div className="p-4 rounded-xl bg-atom-gold/10 border border-atom-gold/20">
              <div className="flex items-center gap-3">
                <div className="p-2 rounded-lg bg-atom-gold/20">
                  <Wallet className="w-5 h-5 text-atom-gold" />
                </div>
                <div>
                  <p className="text-atom-gold font-medium">Connect Wallet</p>
                  <p className="text-sm text-space-400">
                    Connect a wallet to submit intents
                  </p>
                </div>
              </div>
            </div>
          ) : (
            <button
              type="submit"
              disabled={mutation.isPending || !inputAmount || parseFloat(inputAmount) <= 0 || hasInsufficientBalance}
              className="w-full btn-primary py-3.5 text-base disabled:opacity-50 disabled:cursor-not-allowed"
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
            <div className="p-3 rounded-xl bg-red-500/10 border border-red-500/20">
              <p className="text-red-400 text-sm text-center">
                Error: {(mutation.error as Error).message}
              </p>
            </div>
          )}
        </form>

        {/* Intent Preview */}
        <div className="card space-y-6">
          <h3 className="text-lg font-semibold text-white">Intent Preview</h3>

          {/* Trade Summary */}
          <div className="p-5 rounded-xl bg-space-900/60 border border-white/5">
            <p className="text-sm text-space-400 mb-4">Trade Summary</p>
            <div className="flex items-center justify-between">
              <div className="text-center">
                {getTokenIcon(inputDenom)}
                <p className="text-white font-semibold mt-2">{inputAmount} {inputDenom}</p>
                <p className="text-xs text-space-500">{TOKENS[inputDenom]?.name}</p>
              </div>
              <div className="flex-1 flex justify-center">
                <div className="w-12 h-[2px] bg-gradient-to-r from-cosmos-500 to-atom-cyan" />
              </div>
              <div className="text-center">
                {getTokenIcon(outputDenom)}
                <p className="text-white font-semibold mt-2">{estimatedOutput.toFixed(4)} {outputDenom}</p>
                <p className="text-xs text-space-500">{TOKENS[outputDenom]?.name}</p>
              </div>
            </div>
          </div>

          {/* Execution Details */}
          <div className="p-5 rounded-xl bg-space-900/60 border border-white/5">
            <p className="text-sm text-space-400 mb-4">Execution Details</p>
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <p className="text-space-500">Source Chain</p>
                <p className="text-white font-medium">{TOKENS[inputDenom]?.chain}</p>
              </div>
              <div>
                <p className="text-space-500">Destination Chain</p>
                <p className="text-white font-medium">{TOKENS[outputDenom]?.chain}</p>
              </div>
              <div>
                <p className="text-space-500">Max Slippage</p>
                <p className="text-white font-medium">{slippage}%</p>
              </div>
              <div>
                <p className="text-space-500">Timeout</p>
                <p className="text-white font-medium">60 seconds</p>
              </div>
            </div>
          </div>

          {/* How it works */}
          <div className="p-5 rounded-xl bg-cosmos-500/10 border border-cosmos-500/20">
            <div className="flex items-start gap-3">
              <Info className="w-5 h-5 text-cosmos-400 mt-0.5 flex-shrink-0" />
              <div>
                <p className="text-sm font-medium text-cosmos-300 mb-2">How it works</p>
                <ol className="text-sm text-space-400 space-y-1.5">
                  <li className="flex items-start gap-2">
                    <span className="text-cosmos-400 font-medium">1.</span>
                    Your intent is broadcast to all solvers
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-cosmos-400 font-medium">2.</span>
                    Solvers compete in a batch auction
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-cosmos-400 font-medium">3.</span>
                    Best price wins, funds are escrowed
                  </li>
                  <li className="flex items-start gap-2">
                    <span className="text-cosmos-400 font-medium">4.</span>
                    Settlement via IBC (2-5 seconds)
                  </li>
                </ol>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
