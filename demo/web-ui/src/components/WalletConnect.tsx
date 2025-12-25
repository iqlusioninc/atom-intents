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
          {connecting ? 'Connecting...' : 'Connect Wallet'}
          <ChevronDown className="w-4 h-4" />
        </button>

        {showDropdown && (
          <div className="absolute right-0 mt-2 w-64 bg-gray-800 rounded-lg shadow-xl border border-gray-700 z-50">
            <div className="p-2">
              <button
                onClick={() => handleConnect('demo')}
                className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-gray-700 transition-colors"
              >
                <div className="w-8 h-8 rounded-full bg-cosmos-600 flex items-center justify-center">
                  <span className="text-sm">D</span>
                </div>
                <div className="text-left">
                  <p className="text-white font-medium">Demo Wallet</p>
                  <p className="text-xs text-gray-400">Pre-funded test wallet</p>
                </div>
              </button>

              <button
                onClick={() => handleConnect('keplr')}
                className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-gray-700 transition-colors"
              >
                <div className="w-8 h-8 rounded-full bg-blue-600 flex items-center justify-center">
                  <span className="text-sm">K</span>
                </div>
                <div className="text-left">
                  <p className="text-white font-medium">Keplr</p>
                  <p className="text-xs text-gray-400">Simulated connection</p>
                </div>
              </button>

              <button
                onClick={() => handleConnect('leap')}
                className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-gray-700 transition-colors"
              >
                <div className="w-8 h-8 rounded-full bg-green-600 flex items-center justify-center">
                  <span className="text-sm">L</span>
                </div>
                <div className="text-left">
                  <p className="text-white font-medium">Leap</p>
                  <p className="text-xs text-gray-400">Simulated connection</p>
                </div>
              </button>
            </div>

            <div className="border-t border-gray-700 p-3">
              <p className="text-xs text-gray-500 text-center">
                Demo mode uses simulated wallets with test balances
              </p>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="relative">
      <button
        onClick={() => setShowDropdown(!showDropdown)}
        className="flex items-center gap-3 px-4 py-2 bg-gray-800 hover:bg-gray-700 rounded-lg transition-colors"
      >
        <div className="w-8 h-8 rounded-full bg-cosmos-600 flex items-center justify-center">
          <Wallet className="w-4 h-4 text-white" />
        </div>
        <div className="text-left">
          <p className="text-sm text-white font-medium">{formatAddress(address!)}</p>
          <p className="text-xs text-gray-400">{name}</p>
        </div>
        <ChevronDown className="w-4 h-4 text-gray-400" />
      </button>

      {showDropdown && (
        <div className="absolute right-0 mt-2 w-72 bg-gray-800 rounded-lg shadow-xl border border-gray-700 z-50">
          {/* Address */}
          <div className="p-4 border-b border-gray-700">
            <div className="flex items-center justify-between">
              <p className="text-sm text-gray-400">Address</p>
              <button
                onClick={handleCopy}
                className="text-gray-400 hover:text-white transition-colors"
              >
                {copied ? (
                  <Check className="w-4 h-4 text-green-400" />
                ) : (
                  <Copy className="w-4 h-4" />
                )}
              </button>
            </div>
            <p className="text-sm text-white font-mono mt-1">{formatAddress(address!)}</p>
          </div>

          {/* Balances */}
          <div className="p-4 border-b border-gray-700">
            <p className="text-sm text-gray-400 mb-3">Balances</p>
            <div className="space-y-2">
              {Object.entries(balances).map(([denom, amount]) => (
                <div key={denom} className="flex items-center justify-between">
                  <span className="text-white">{denom}</span>
                  <span className="text-gray-300 font-mono">
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
              className="w-full flex items-center justify-center gap-2 p-3 text-red-400 hover:bg-red-900/20 rounded-lg transition-colors"
            >
              <LogOut className="w-4 h-4" />
              Disconnect
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
