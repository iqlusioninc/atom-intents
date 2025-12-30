import { useState, useEffect } from 'react';
import { Wallet, LogOut, Copy, Check, ChevronDown, ExternalLink, RefreshCw, AlertCircle } from 'lucide-react';
import { useWallet, type WalletType } from '../hooks/useWallet';
import { TESTNET_FAUCET_URL, getAddressUrl } from '../config/chains';

export default function WalletConnect() {
  const {
    connected,
    status,
    address,
    name,
    walletType,
    balances,
    error,
    connect,
    disconnect,
    refreshBalance,
    isKeplrAvailable,
  } = useWallet();

  const [showDropdown, setShowDropdown] = useState(false);
  const [copied, setCopied] = useState(false);
  const [keplrInstalled, setKeplrInstalled] = useState(false);

  // Check for Keplr on mount
  useEffect(() => {
    const checkKeplr = () => {
      setKeplrInstalled(isKeplrAvailable());
    };
    checkKeplr();
    // Re-check after a delay (Keplr might load async)
    const timer = setTimeout(checkKeplr, 1000);
    return () => clearTimeout(timer);
  }, [isKeplrAvailable]);

  const handleConnect = async (type: WalletType) => {
    try {
      await connect(type);
      setShowDropdown(false);
    } catch (err) {
      // Error is stored in state, will be shown in UI
      console.error('Connection error:', err);
    }
  };

  const handleCopy = () => {
    if (address) {
      navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleRefreshBalance = async () => {
    await refreshBalance();
  };

  const formatAddress = (addr: string) => {
    return `${addr.slice(0, 10)}...${addr.slice(-6)}`;
  };

  const formatBalance = (amount: number) => {
    return (amount / 1_000_000).toLocaleString(undefined, {
      minimumFractionDigits: 2,
      maximumFractionDigits: 6,
    });
  };

  const isConnecting = status === 'connecting';

  if (!connected) {
    return (
      <div className="relative">
        <button
          onClick={() => setShowDropdown(!showDropdown)}
          disabled={isConnecting}
          className="btn-primary flex items-center gap-1 sm:gap-2 text-sm sm:text-base px-2 sm:px-4 py-1.5 sm:py-2"
        >
          <Wallet className="w-4 h-4" />
          <span className="hidden xs:inline">{isConnecting ? 'Connecting...' : 'Connect'}</span>
          <span className="hidden sm:inline">{isConnecting ? '' : ' Wallet'}</span>
          <ChevronDown className="w-4 h-4 hidden sm:block" />
        </button>

        {showDropdown && (
          <>
            {/* Mobile overlay */}
            <div
              className="fixed inset-0 z-40 sm:hidden"
              onClick={() => setShowDropdown(false)}
            />
            <div className="fixed sm:absolute left-4 right-4 sm:left-auto sm:right-0 bottom-4 sm:bottom-auto sm:mt-2 sm:w-72 bg-gray-800 rounded-lg shadow-xl border border-gray-700 z-50">
              {/* Error message */}
              {error && (
                <div className="p-3 bg-red-900/30 border-b border-red-700/50">
                  <div className="flex items-start gap-2">
                    <AlertCircle className="w-4 h-4 text-red-400 flex-shrink-0 mt-0.5" />
                    <p className="text-xs text-red-300">{error}</p>
                  </div>
                </div>
              )}

              <div className="p-2">
                {/* Keplr - Real connection */}
                <button
                  onClick={() => handleConnect('keplr')}
                  disabled={!keplrInstalled}
                  className={`w-full flex items-center gap-3 p-3 rounded-lg transition-colors ${
                    keplrInstalled
                      ? 'hover:bg-gray-700 active:bg-gray-600'
                      : 'opacity-50 cursor-not-allowed'
                  }`}
                >
                  <div className="w-8 h-8 rounded-full bg-blue-600 flex items-center justify-center flex-shrink-0">
                    <span className="text-sm font-bold">K</span>
                  </div>
                  <div className="text-left flex-1">
                    <p className="text-white font-medium">Keplr Wallet</p>
                    <p className="text-xs text-gray-400">
                      {keplrInstalled ? 'Connect to provider testnet' : 'Not installed'}
                    </p>
                  </div>
                  {keplrInstalled && (
                    <span className="text-xs bg-green-600/20 text-green-400 px-2 py-0.5 rounded">
                      Testnet
                    </span>
                  )}
                </button>

                {!keplrInstalled && (
                  <a
                    href="https://www.keplr.app/download"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="w-full flex items-center justify-center gap-2 p-2 text-xs text-cosmos-400 hover:text-cosmos-300 transition-colors"
                  >
                    <ExternalLink className="w-3 h-3" />
                    Install Keplr Extension
                  </a>
                )}

                <div className="my-2 border-t border-gray-700" />

                {/* Demo mode */}
                <button
                  onClick={() => handleConnect('demo')}
                  className="w-full flex items-center gap-3 p-3 rounded-lg hover:bg-gray-700 transition-colors active:bg-gray-600"
                >
                  <div className="w-8 h-8 rounded-full bg-cosmos-600 flex items-center justify-center flex-shrink-0">
                    <span className="text-sm font-bold">D</span>
                  </div>
                  <div className="text-left flex-1">
                    <p className="text-white font-medium">Demo Mode</p>
                    <p className="text-xs text-gray-400">Simulated wallet with test funds</p>
                  </div>
                  <span className="text-xs bg-gray-600/50 text-gray-300 px-2 py-0.5 rounded">
                    Demo
                  </span>
                </button>
              </div>

              <div className="border-t border-gray-700 p-3">
                <p className="text-xs text-gray-500 text-center">
                  Keplr connects to Cosmos Hub testnet (provider chain)
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
        className="flex items-center gap-2 sm:gap-3 px-2 sm:px-4 py-1.5 sm:py-2 bg-gray-800 hover:bg-gray-700 rounded-lg transition-colors"
      >
        <div className={`w-6 h-6 sm:w-8 sm:h-8 rounded-full flex items-center justify-center flex-shrink-0 ${
          walletType === 'keplr' ? 'bg-blue-600' : 'bg-cosmos-600'
        }`}>
          <Wallet className="w-3 h-3 sm:w-4 sm:h-4 text-white" />
        </div>
        <div className="text-left hidden xs:block">
          <p className="text-xs sm:text-sm text-white font-medium">{formatAddress(address!)}</p>
          <p className="text-xs text-gray-400 hidden sm:block">
            {name}
            {walletType === 'keplr' && <span className="text-green-400 ml-1">(Testnet)</span>}
          </p>
        </div>
        <ChevronDown className="w-4 h-4 text-gray-400 hidden sm:block" />
      </button>

      {showDropdown && (
        <>
          {/* Mobile overlay */}
          <div
            className="fixed inset-0 z-40 sm:hidden"
            onClick={() => setShowDropdown(false)}
          />
          <div className="fixed sm:absolute left-4 right-4 sm:left-auto sm:right-0 bottom-4 sm:bottom-auto sm:mt-2 sm:w-80 bg-gray-800 rounded-lg shadow-xl border border-gray-700 z-50">
            {/* Connection type badge */}
            <div className="px-4 pt-4 pb-2">
              <span className={`text-xs px-2 py-1 rounded ${
                walletType === 'keplr'
                  ? 'bg-blue-600/20 text-blue-400'
                  : 'bg-gray-600/50 text-gray-400'
              }`}>
                {walletType === 'keplr' ? 'Keplr - Provider Testnet' : 'Demo Mode'}
              </span>
            </div>

            {/* Address */}
            <div className="p-4 border-b border-gray-700">
              <div className="flex items-center justify-between">
                <p className="text-sm text-gray-400">Address</p>
                <div className="flex items-center gap-2">
                  {walletType === 'keplr' && (
                    <a
                      href={getAddressUrl(address!)}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-gray-400 hover:text-white transition-colors p-1"
                      title="View on explorer"
                    >
                      <ExternalLink className="w-4 h-4" />
                    </a>
                  )}
                  <button
                    onClick={handleCopy}
                    className="text-gray-400 hover:text-white transition-colors p-1"
                  >
                    {copied ? (
                      <Check className="w-4 h-4 text-green-400" />
                    ) : (
                      <Copy className="w-4 h-4" />
                    )}
                  </button>
                </div>
              </div>
              <p className="text-sm text-white font-mono mt-1 break-all">{formatAddress(address!)}</p>
            </div>

            {/* Balances */}
            <div className="p-4 border-b border-gray-700">
              <div className="flex items-center justify-between mb-3">
                <p className="text-sm text-gray-400">Balances</p>
                {walletType === 'keplr' && (
                  <button
                    onClick={handleRefreshBalance}
                    className="text-gray-400 hover:text-white transition-colors p-1"
                    title="Refresh balances"
                  >
                    <RefreshCw className="w-4 h-4" />
                  </button>
                )}
              </div>
              <div className="space-y-2">
                {Object.keys(balances).length === 0 ? (
                  <p className="text-sm text-gray-500 italic">No balances</p>
                ) : (
                  Object.entries(balances).map(([denom, amount]) => (
                    <div key={denom} className="flex items-center justify-between">
                      <span className="text-white">{denom}</span>
                      <span className="text-gray-300 font-mono">
                        {formatBalance(amount)}
                      </span>
                    </div>
                  ))
                )}
              </div>

              {/* Faucet link for Keplr users */}
              {walletType === 'keplr' && (
                <a
                  href={TESTNET_FAUCET_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center justify-center gap-2 mt-4 text-xs text-cosmos-400 hover:text-cosmos-300 transition-colors"
                >
                  <ExternalLink className="w-3 h-3" />
                  Get testnet ATOM from faucet
                </a>
              )}
            </div>

            {/* Disconnect */}
            <div className="p-2">
              <button
                onClick={() => {
                  disconnect();
                  setShowDropdown(false);
                }}
                className="w-full flex items-center justify-center gap-2 p-3 text-red-400 hover:bg-red-900/20 active:bg-red-900/30 rounded-lg transition-colors"
              >
                <LogOut className="w-4 h-4" />
                Disconnect
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
