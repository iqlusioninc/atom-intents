import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowRight, Loader2, Check, Wallet, AlertCircle, Settings, ChevronDown, ChevronUp } from 'lucide-react';
import * as api from '../services/api';
import { useStore } from '../hooks/useStore';
import { useWallet } from '../hooks/useWallet';
import { TOKENS } from '../types';

type FillStrategy = 'eager' | 'all_or_nothing' | 'time_based' | 'price_based';

export default function IntentCreator() {
  const prices = useStore((state) => state.prices);
  const addIntent = useStore((state) => state.addIntent);
  const { connected, address, balances, updateBalance } = useWallet();

  const [inputDenom, setInputDenom] = useState('ATOM');
  const [outputDenom, setOutputDenom] = useState('OSMO');
  const [inputAmount, setInputAmount] = useState('10');
  const [slippage, setSlippage] = useState('1');
  const [success, setSuccess] = useState(false);

  // Partial fill configuration
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [allowPartial, setAllowPartial] = useState(true);
  const [minFillPercent, setMinFillPercent] = useState(80);
  const [fillStrategy, setFillStrategy] = useState<FillStrategy>('eager');

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
        allow_partial: allowPartial,
        min_fill_percent: minFillPercent,
        strategy: fillStrategy,
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
    <div className="space-y-4 sm:space-y-6 animate-slide-in">
      <div>
        <h2 className="text-xl sm:text-2xl font-bold text-white">Create Intent</h2>
        <p className="text-gray-400 text-sm sm:text-base">Submit a new trading intent to the system</p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 sm:gap-6">
        {/* Intent Form */}
        <form onSubmit={handleSubmit} className="card space-y-4 sm:space-y-6">
          <div>
            <label className="block text-sm font-medium text-gray-300 mb-2">
              From (Input)
            </label>
            <div className="flex flex-col xs:flex-row gap-2 sm:gap-3">
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
            <div className="flex flex-col xs:flex-row xs:items-center xs:justify-between mt-1 gap-0.5">
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
            <div className="flex flex-col xs:flex-row gap-2 sm:gap-3">
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
            <div className="flex gap-1 sm:gap-2">
              {['0.5', '1', '2', '5'].map((val) => (
                <button
                  key={val}
                  type="button"
                  onClick={() => setSlippage(val)}
                  className={`px-3 sm:px-4 py-2 rounded-lg transition-colors flex-1 text-sm sm:text-base min-h-[44px] ${
                    slippage === val
                      ? 'bg-cosmos-600 text-white'
                      : 'bg-gray-800 text-gray-400 hover:bg-gray-700 active:bg-gray-600'
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

          {/* Advanced Settings - Partial Fill Configuration */}
          <div className="border-t border-gray-700 pt-4">
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="flex items-center gap-2 text-sm text-gray-400 hover:text-white transition-colors w-full"
            >
              <Settings className="w-4 h-4" />
              <span>Advanced Settings</span>
              {showAdvanced ? <ChevronUp className="w-4 h-4 ml-auto" /> : <ChevronDown className="w-4 h-4 ml-auto" />}
            </button>

            {showAdvanced && (
              <div className="mt-4 space-y-4 p-4 bg-gray-800/50 rounded-lg">
                {/* Partial Fill Toggle */}
                <div className="flex items-center justify-between">
                  <div>
                    <label className="text-sm font-medium text-gray-300">Allow Partial Fills</label>
                    <p className="text-xs text-gray-500">Accept orders that only partially fill your intent</p>
                  </div>
                  <button
                    type="button"
                    onClick={() => setAllowPartial(!allowPartial)}
                    className={`relative w-12 h-6 rounded-full transition-colors ${
                      allowPartial ? 'bg-cosmos-600' : 'bg-gray-600'
                    }`}
                  >
                    <span
                      className={`absolute top-1 w-4 h-4 bg-white rounded-full transition-transform ${
                        allowPartial ? 'left-7' : 'left-1'
                      }`}
                    />
                  </button>
                </div>

                {/* Min Fill Percentage (only show if partial fills enabled) */}
                {allowPartial && (
                  <div>
                    <div className="flex items-center justify-between mb-2">
                      <label className="text-sm font-medium text-gray-300">Minimum Fill %</label>
                      <span className="text-sm text-cosmos-400 font-medium">{minFillPercent}%</span>
                    </div>
                    <input
                      type="range"
                      min="10"
                      max="100"
                      value={minFillPercent}
                      onChange={(e) => setMinFillPercent(parseInt(e.target.value))}
                      className="w-full h-2 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-cosmos-500"
                    />
                    <div className="flex justify-between text-xs text-gray-500 mt-1">
                      <span>10%</span>
                      <span>50%</span>
                      <span>100%</span>
                    </div>
                  </div>
                )}

                {/* Fill Strategy */}
                <div>
                  <label className="block text-sm font-medium text-gray-300 mb-2">Fill Strategy</label>
                  <select
                    value={fillStrategy}
                    onChange={(e) => setFillStrategy(e.target.value as FillStrategy)}
                    className="select w-full"
                    disabled={!allowPartial && fillStrategy !== 'all_or_nothing'}
                  >
                    <option value="eager">Eager - Accept any fill meeting price</option>
                    <option value="all_or_nothing">All or Nothing - 100% fill required</option>
                    <option value="time_based">Time-based - Wait for better fills</option>
                    <option value="price_based">Price-based - Optimize for best price</option>
                  </select>
                  <p className="text-xs text-gray-500 mt-1">
                    {fillStrategy === 'eager' && 'Accept fills as soon as they meet your minimum requirements'}
                    {fillStrategy === 'all_or_nothing' && 'Only accept if entire order can be filled (no partial fills)'}
                    {fillStrategy === 'time_based' && 'Wait for aggregation window to find more fills'}
                    {fillStrategy === 'price_based' && 'Wait for optimal price across multiple solvers'}
                  </p>
                </div>
              </div>
            )}
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
        <div className="card space-y-3 sm:space-y-4">
          <h3 className="text-base sm:text-lg font-semibold text-white">Intent Preview</h3>

          <div className="space-y-3 sm:space-y-4">
            <div className="p-3 sm:p-4 bg-gray-800/50 rounded-lg">
              <p className="text-sm text-gray-400 mb-2">Trade Summary</p>
              <div className="flex items-center gap-2 sm:gap-3 justify-center">
                <div className="text-center min-w-0 flex-1">
                  <p className="text-xl sm:text-2xl mb-1">{TOKENS[inputDenom]?.logo}</p>
                  <p className="text-white font-medium text-sm sm:text-base truncate">{inputAmount} {inputDenom}</p>
                  <p className="text-xs text-gray-400 truncate">{TOKENS[inputDenom]?.name}</p>
                </div>
                <ArrowRight className="w-5 h-5 sm:w-6 sm:h-6 text-cosmos-400 flex-shrink-0" />
                <div className="text-center min-w-0 flex-1">
                  <p className="text-xl sm:text-2xl mb-1">{TOKENS[outputDenom]?.logo}</p>
                  <p className="text-white font-medium text-sm sm:text-base truncate">{estimatedOutput.toFixed(4)} {outputDenom}</p>
                  <p className="text-xs text-gray-400 truncate">{TOKENS[outputDenom]?.name}</p>
                </div>
              </div>
            </div>

            <div className="p-3 sm:p-4 bg-gray-800/50 rounded-lg space-y-2">
              <p className="text-sm text-gray-400">Execution Details</p>
              <div className="grid grid-cols-2 gap-2 sm:gap-4 text-xs sm:text-sm">
                <div>
                  <p className="text-gray-500">Source Chain</p>
                  <p className="text-white truncate">{TOKENS[inputDenom]?.chain}</p>
                </div>
                <div>
                  <p className="text-gray-500">Dest Chain</p>
                  <p className="text-white truncate">{TOKENS[outputDenom]?.chain}</p>
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

            {/* Partial Fill Settings Preview */}
            <div className="p-3 sm:p-4 bg-gray-800/50 rounded-lg space-y-2">
              <p className="text-sm text-gray-400">Fill Settings</p>
              <div className="grid grid-cols-2 gap-2 sm:gap-4 text-xs sm:text-sm">
                <div>
                  <p className="text-gray-500">Partial Fills</p>
                  <p className={allowPartial ? 'text-green-400' : 'text-yellow-400'}>
                    {allowPartial ? 'Enabled' : 'Disabled'}
                  </p>
                </div>
                <div>
                  <p className="text-gray-500">Min Fill</p>
                  <p className="text-white">{allowPartial ? `${minFillPercent}%` : '100%'}</p>
                </div>
                <div className="col-span-2">
                  <p className="text-gray-500">Strategy</p>
                  <p className="text-white capitalize">{fillStrategy.replace('_', ' ')}</p>
                </div>
              </div>
            </div>

            <div className="p-3 sm:p-4 bg-cosmos-900/30 border border-cosmos-700/50 rounded-lg">
              <p className="text-sm text-cosmos-300 mb-2">How it works:</p>
              <ol className="text-xs sm:text-sm text-gray-400 space-y-1">
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
