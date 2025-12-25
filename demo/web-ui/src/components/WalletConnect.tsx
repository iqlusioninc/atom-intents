import { useState } from 'react';
import { Wallet, LogOut, Copy, Check, ChevronDown } from 'lucide-react';
import { useWallet } from '../hooks/useWallet';

export default function WalletConnect() {
  const { connected, address, name, balances, connect, disconnect } = useWallet();
  const [showDropdown, setShowDropdown] = useState(false);
  const [copied, setCopied] = useState(false);
  const [connecting, setConnecting] = useState(false);

  const handleConnect = async (type: 'keplr' | 'leap' | 'demo') => {
    setConnecting(true);
    try {
      await connect(type);
    } finally {
      setConnecting(false);
      setShowDropdown(false);
    }
  };

  const handleCopy = () => {
    if (address) {
      navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const formatAddress = (addr: string) => {
    return `${addr.slice(0, 10)}...${addr.slice(-6)}`;
  };

  const formatBalance = (amount: number) => {
    return (amount / 1_000_000).toLocaleString(undefined, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  };

  if (!connected) {
    return (
      <div className="relative">
        <button
          onClick={() => setShowDropdown(!showDropdown)}
          disabled={connecting}
          className="btn-primary flex items-center gap-2"
        >
          <Wallet className="w-4 h-4" />
          {connecting ? 'Connecting...' : 'Connect'}
          <ChevronDown className="w-4 h-4" />
        </button>

        {showDropdown && (
          <>
            {/* Backdrop */}
            <div
              className="fixed inset-0 z-40"
              onClick={() => setShowDropdown(false)}
            />

            {/* Dropdown */}
            <div className="absolute right-0 mt-2 w-72 bg-space-900 rounded-2xl shadow-xl border border-white/10 z-50 overflow-hidden">
              <div className="p-2">
                <button
                  onClick={() => handleConnect('demo')}
                  className="w-full flex items-center gap-3 p-3 rounded-xl hover:bg-white/5 transition-colors"
                >
                  <div className="w-10 h-10 rounded-full bg-gradient-to-br from-cosmos-500 to-cosmos-600 flex items-center justify-center">
                    <span className="text-white font-bold">D</span>
                  </div>
                  <div className="text-left">
                    <p className="text-white font-medium">Demo Wallet</p>
                    <p className="text-xs text-space-400">Pre-funded test wallet</p>
                  </div>
                </button>

                <button
                  onClick={() => handleConnect('keplr')}
                  className="w-full flex items-center gap-3 p-3 rounded-xl hover:bg-white/5 transition-colors"
                >
                  <div className="w-10 h-10 rounded-full bg-gradient-to-br from-blue-500 to-blue-600 flex items-center justify-center">
                    <span className="text-white font-bold">K</span>
                  </div>
                  <div className="text-left">
                    <p className="text-white font-medium">Keplr</p>
                    <p className="text-xs text-space-400">Simulated connection</p>
                  </div>
                </button>

                <button
                  onClick={() => handleConnect('leap')}
                  className="w-full flex items-center gap-3 p-3 rounded-xl hover:bg-white/5 transition-colors"
                >
                  <div className="w-10 h-10 rounded-full bg-gradient-to-br from-emerald-500 to-emerald-600 flex items-center justify-center">
                    <span className="text-white font-bold">L</span>
                  </div>
                  <div className="text-left">
                    <p className="text-white font-medium">Leap</p>
                    <p className="text-xs text-space-400">Simulated connection</p>
                  </div>
                </button>
              </div>

              <div className="border-t border-white/5 p-3">
                <p className="text-xs text-space-500 text-center">
                  Demo mode uses simulated wallets with test balances
                </p>
              </div>
            </div>
          </>
        )}
      </div>
    );
  }

  return (
    <div className="relative">
      <button
        onClick={() => setShowDropdown(!showDropdown)}
        className="flex items-center gap-3 px-3 py-2 bg-space-800/80 hover:bg-space-700/80 rounded-xl border border-white/5 transition-all"
      >
        <div className="w-8 h-8 rounded-full bg-gradient-to-br from-cosmos-500 to-cosmos-600 flex items-center justify-center">
          <Wallet className="w-4 h-4 text-white" />
        </div>
        <div className="text-left">
          <p className="text-sm text-white font-medium">{formatAddress(address!)}</p>
          <p className="text-xs text-space-400">{name}</p>
        </div>
        <ChevronDown className="w-4 h-4 text-space-400" />
      </button>

      {showDropdown && (
        <>
          {/* Backdrop */}
          <div
            className="fixed inset-0 z-40"
            onClick={() => setShowDropdown(false)}
          />

          {/* Dropdown */}
          <div className="absolute right-0 mt-2 w-80 bg-space-900 rounded-2xl shadow-xl border border-white/10 z-50 overflow-hidden">
            {/* Address */}
            <div className="p-4 border-b border-white/5">
              <div className="flex items-center justify-between mb-1">
                <p className="text-sm text-space-400">Address</p>
                <button
                  onClick={handleCopy}
                  className="p-1.5 rounded-lg text-space-400 hover:text-white hover:bg-white/5 transition-colors"
                >
                  {copied ? (
                    <Check className="w-4 h-4 text-atom-green" />
                  ) : (
                    <Copy className="w-4 h-4" />
                  )}
                </button>
              </div>
              <p className="text-sm text-white font-mono bg-space-800/80 px-3 py-2 rounded-lg">
                {formatAddress(address!)}
              </p>
            </div>

            {/* Balances */}
            <div className="p-4 border-b border-white/5">
              <p className="text-sm text-space-400 mb-3">Balances</p>
              <div className="space-y-2">
                {Object.entries(balances).map(([denom, amount]) => (
                  <div
                    key={denom}
                    className="flex items-center justify-between p-2 rounded-lg bg-space-800/40"
                  >
                    <div className="flex items-center gap-2">
                      <div className="w-6 h-6 rounded-full bg-cosmos-500/20 flex items-center justify-center">
                        <span className="text-cosmos-400 text-xs font-bold">{denom[0]}</span>
                      </div>
                      <span className="text-white font-medium text-sm">{denom}</span>
                    </div>
                    <span className="text-space-300 font-mono text-sm">
                      {formatBalance(amount)}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Disconnect */}
            <div className="p-2">
              <button
                onClick={() => {
                  disconnect();
                  setShowDropdown(false);
                }}
                className="w-full flex items-center justify-center gap-2 p-3 text-red-400 hover:bg-red-500/10 rounded-xl transition-colors"
              >
                <LogOut className="w-4 h-4" />
                <span className="font-medium">Disconnect</span>
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
