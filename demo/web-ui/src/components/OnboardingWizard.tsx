import { useState, useEffect } from 'react';
import { X, ChevronLeft, ChevronRight, Zap, Users, Shield, ArrowRightLeft, Globe, Rocket } from 'lucide-react';

const STORAGE_KEY = 'atom-intents-onboarding-seen';

interface OnboardingWizardProps {
  isOpen: boolean;
  onClose: () => void;
}

interface Slide {
  icon: React.ReactNode;
  title: string;
  subtitle?: string;
  content: React.ReactNode;
}

const slides: Slide[] = [
  {
    icon: <span className="text-5xl">⚛️</span>,
    title: 'Welcome to ATOM Intents',
    subtitle: 'Intent-Based Liquidity for Cosmos Hub',
    content: (
      <div className="space-y-4">
        <p>
          This demo showcases a new paradigm for cross-chain trading in the Cosmos ecosystem.
        </p>
        <p>
          Instead of routing through DEXs yourself, you simply express <strong className="text-cosmos-400">what you want</strong> —
          and let professional solvers compete to give you the best execution.
        </p>
        <div className="mt-6 p-4 bg-cosmos-900/30 rounded-lg border border-cosmos-700/50">
          <p className="text-sm text-cosmos-300">
            Built on <strong>Skip Select</strong> — bringing MEV protection and optimal execution to Cosmos Hub.
          </p>
        </div>
      </div>
    ),
  },
  {
    icon: <Zap className="w-12 h-12 text-yellow-400" />,
    title: 'What Are Intents?',
    subtitle: 'Express what you want, not how to get it',
    content: (
      <div className="space-y-4">
        <p>
          An <strong className="text-cosmos-400">intent</strong> is a signed message that says:
        </p>
        <div className="bg-gray-800/50 rounded-lg p-4 font-mono text-sm">
          <p>"I want to swap <span className="text-green-400">100 ATOM</span> for at least <span className="text-blue-400">145 OSMO</span>"</p>
        </div>
        <p>
          You don't specify the route, the DEX, or the exact price — you just set your minimum acceptable output.
        </p>
        <div className="grid grid-cols-1 xs:grid-cols-2 gap-3 sm:gap-4 mt-4">
          <div className="p-3 bg-red-900/20 rounded-lg border border-red-700/50">
            <p className="text-red-400 font-medium text-sm">Traditional Trading</p>
            <p className="text-xs text-gray-400 mt-1">You find routes, compare prices, execute transactions</p>
          </div>
          <div className="p-3 bg-green-900/20 rounded-lg border border-green-700/50">
            <p className="text-green-400 font-medium text-sm">Intent-Based</p>
            <p className="text-xs text-gray-400 mt-1">You state what you want, solvers compete to fill it</p>
          </div>
        </div>
      </div>
    ),
  },
  {
    icon: <Users className="w-12 h-12 text-blue-400" />,
    title: 'Solver Competition',
    subtitle: 'Batch auctions ensure fair pricing',
    content: (
      <div className="space-y-4">
        <p>
          When you submit an intent, it enters a <strong className="text-cosmos-400">batch auction</strong> where
          multiple solvers compete to fill your order.
        </p>
        <div className="relative py-4 sm:py-6">
          <div className="flex items-center justify-between gap-1 sm:gap-2">
            <div className="text-center flex-1">
              <div className="w-12 h-12 sm:w-16 sm:h-16 rounded-full bg-cosmos-600 flex items-center justify-center mx-auto mb-1 sm:mb-2">
                <span className="text-xl sm:text-2xl">👤</span>
              </div>
              <p className="text-[10px] sm:text-xs text-gray-400">Your Intent</p>
            </div>
            <div className="flex items-center justify-center">
              <ArrowRightLeft className="w-4 h-4 sm:w-6 sm:h-6 text-gray-500" />
            </div>
            <div className="text-center flex-1">
              <div className="w-12 h-12 sm:w-16 sm:h-16 rounded-full bg-yellow-600 flex items-center justify-center mx-auto mb-1 sm:mb-2">
                <span className="text-xl sm:text-2xl">🏆</span>
              </div>
              <p className="text-[10px] sm:text-xs text-gray-400">Batch Auction</p>
            </div>
            <div className="flex items-center justify-center">
              <ArrowRightLeft className="w-4 h-4 sm:w-6 sm:h-6 text-gray-500" />
            </div>
            <div className="text-center flex-1">
              <div className="w-12 h-12 sm:w-16 sm:h-16 rounded-full bg-green-600 flex items-center justify-center mx-auto mb-1 sm:mb-2">
                <span className="text-xl sm:text-2xl">✓</span>
              </div>
              <p className="text-[10px] sm:text-xs text-gray-400">Best Price</p>
            </div>
          </div>
        </div>
        <p className="text-sm text-gray-400">
          Solvers can match intents directly, route through DEXs, or use off-chain liquidity —
          whatever gets you the best price.
        </p>
      </div>
    ),
  },
  {
    icon: <Shield className="w-12 h-12 text-green-400" />,
    title: 'Secure Settlement',
    subtitle: 'Your funds are protected by escrow',
    content: (
      <div className="space-y-4">
        <p>
          Funds are locked in an <strong className="text-cosmos-400">escrow contract</strong> on Cosmos Hub
          until the solver delivers your tokens.
        </p>
        <div className="space-y-3 mt-4">
          <div className="flex items-center gap-3 p-3 bg-gray-800/50 rounded-lg">
            <div className="w-8 h-8 rounded-full bg-blue-600 flex items-center justify-center text-sm font-bold">1</div>
            <p className="text-sm">You lock ATOM in Hub escrow</p>
          </div>
          <div className="flex items-center gap-3 p-3 bg-gray-800/50 rounded-lg">
            <div className="w-8 h-8 rounded-full bg-yellow-600 flex items-center justify-center text-sm font-bold">2</div>
            <p className="text-sm">Solver sends OSMO to you (verified via IBC)</p>
          </div>
          <div className="flex items-center gap-3 p-3 bg-gray-800/50 rounded-lg">
            <div className="w-8 h-8 rounded-full bg-green-600 flex items-center justify-center text-sm font-bold">3</div>
            <p className="text-sm">Hub releases ATOM to solver</p>
          </div>
        </div>
        <p className="text-xs text-gray-500 mt-4">
          If the solver fails to deliver, your funds are automatically refunded after timeout.
        </p>
      </div>
    ),
  },
  {
    icon: <Globe className="w-12 h-12 text-purple-400" />,
    title: 'Cross-Chain Magic',
    subtitle: 'Works even on chains without smart contracts',
    content: (
      <div className="space-y-4">
        <p>
          Using <strong className="text-purple-400">IBC Hooks</strong>, even chains like Celestia (with no smart contracts)
          can participate.
        </p>
        <div className="p-4 bg-purple-900/20 rounded-lg border border-purple-700/50 mt-4">
          <p className="text-purple-300 font-medium mb-2">Example: TIA → USDC</p>
          <div className="text-sm text-gray-400 space-y-2">
            <p>1. Send TIA from Celestia with special IBC memo</p>
            <p>2. Hub escrow locks TIA automatically (<code className="text-purple-300">LockFromIbc</code>)</p>
            <p>3. Solver sends USDC to you on Noble</p>
            <p>4. Hub verifies delivery and releases TIA to solver</p>
          </div>
        </div>
        <p className="text-sm text-gray-400 mt-4">
          The Hub acts as a neutral settlement layer for the entire Cosmos ecosystem.
        </p>
      </div>
    ),
  },
  {
    icon: <Rocket className="w-12 h-12 text-cosmos-400" />,
    title: 'Ready to Explore?',
    subtitle: 'Try out the demo features',
    content: (
      <div className="space-y-4">
        <p>
          This is a live simulation of the ATOM Intents system. Here's what you can do:
        </p>
        <div className="grid grid-cols-2 gap-2 sm:gap-3 mt-4">
          <div className="p-2 sm:p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-xs sm:text-sm">Create Intent</p>
            <p className="text-[10px] sm:text-xs text-gray-400">Submit a swap and watch it fill</p>
          </div>
          <div className="p-2 sm:p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-xs sm:text-sm">Watch Auctions</p>
            <p className="text-[10px] sm:text-xs text-gray-400">See solvers compete live</p>
          </div>
          <div className="p-2 sm:p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-xs sm:text-sm">Track Settlements</p>
            <p className="text-[10px] sm:text-xs text-gray-400">Follow escrow and IBC flow</p>
          </div>
          <div className="p-2 sm:p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-xs sm:text-sm">Run Scenarios</p>
            <p className="text-[10px] sm:text-xs text-gray-400">Try pre-built demo flows</p>
          </div>
        </div>
        <div className="mt-6 p-4 bg-cosmos-900/30 rounded-lg border border-cosmos-700/50 text-center">
          <p className="text-cosmos-300">
            Prices are live from CoinGecko. Everything else is simulated.
          </p>
        </div>
      </div>
    ),
  },
];

export default function OnboardingWizard({ isOpen, onClose }: OnboardingWizardProps) {
  const [currentSlide, setCurrentSlide] = useState(0);

  // Reset to first slide when opening
  useEffect(() => {
    if (isOpen) {
      setCurrentSlide(0);
    }
  }, [isOpen]);

  const handleClose = () => {
    localStorage.setItem(STORAGE_KEY, 'true');
    onClose();
  };

  const nextSlide = () => {
    if (currentSlide < slides.length - 1) {
      setCurrentSlide(currentSlide + 1);
    } else {
      handleClose();
    }
  };

  const prevSlide = () => {
    if (currentSlide > 0) {
      setCurrentSlide(currentSlide - 1);
    }
  };

  if (!isOpen) return null;

  const slide = slides[currentSlide];
  const isLastSlide = currentSlide === slides.length - 1;

  return (
    <div className="fixed inset-0 z-[100] flex items-end sm:items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/80 backdrop-blur-sm"
        onClick={handleClose}
      />

      {/* Modal */}
      <div className="relative w-full sm:max-w-2xl sm:mx-4 bg-gray-900 rounded-t-2xl sm:rounded-2xl border-t sm:border border-gray-700 shadow-2xl overflow-hidden max-h-[90vh] sm:max-h-[85vh] flex flex-col">
        {/* Close button */}
        <button
          onClick={handleClose}
          className="absolute top-3 sm:top-4 right-3 sm:right-4 p-2 text-gray-400 hover:text-white transition-colors z-10"
        >
          <X className="w-5 h-5" />
        </button>

        {/* Content - scrollable */}
        <div className="p-4 sm:p-8 overflow-y-auto flex-1">
          {/* Icon */}
          <div className="flex justify-center mb-4 sm:mb-6">
            <div className="scale-75 sm:scale-100">
              {slide.icon}
            </div>
          </div>

          {/* Title */}
          <h2 className="text-xl sm:text-2xl font-bold text-white text-center mb-1 sm:mb-2">
            {slide.title}
          </h2>
          {slide.subtitle && (
            <p className="text-gray-400 text-center mb-4 sm:mb-6 text-sm sm:text-base">
              {slide.subtitle}
            </p>
          )}

          {/* Content */}
          <div className="text-gray-300 text-sm sm:text-base min-h-[180px] sm:min-h-[250px]">
            {slide.content}
          </div>
        </div>

        {/* Footer - fixed at bottom */}
        <div className="px-4 sm:px-8 pb-4 sm:pb-6 pt-2 border-t border-gray-800 sm:border-0 bg-gray-900 flex-shrink-0">
          {/* Progress dots */}
          <div className="flex justify-center gap-2 mb-4 sm:mb-6">
            {slides.map((_, index) => (
              <button
                key={index}
                onClick={() => setCurrentSlide(index)}
                className={`w-2 h-2 rounded-full transition-colors ${
                  index === currentSlide ? 'bg-cosmos-500' : 'bg-gray-600 hover:bg-gray-500'
                }`}
              />
            ))}
          </div>

          {/* Navigation buttons */}
          <div className="flex justify-between items-center gap-4">
            <button
              onClick={prevSlide}
              disabled={currentSlide === 0}
              className={`flex items-center gap-1 sm:gap-2 px-3 sm:px-4 py-2 rounded-lg transition-colors min-h-[44px] ${
                currentSlide === 0
                  ? 'text-gray-600 cursor-not-allowed'
                  : 'text-gray-400 hover:text-white hover:bg-gray-800 active:bg-gray-700'
              }`}
            >
              <ChevronLeft className="w-4 h-4" />
              <span className="hidden xs:inline">Back</span>
            </button>

            <button
              onClick={nextSlide}
              className="flex items-center gap-1 sm:gap-2 px-4 sm:px-6 py-2 bg-cosmos-600 hover:bg-cosmos-500 active:bg-cosmos-700 text-white rounded-lg transition-colors min-h-[44px]"
            >
              {isLastSlide ? "Let's Go!" : 'Next'}
              {!isLastSlide && <ChevronRight className="w-4 h-4" />}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// Helper hook to manage wizard state
export function useOnboardingWizard() {
  const [isOpen, setIsOpen] = useState(false);
  const [hasSeenOnboarding, setHasSeenOnboarding] = useState(true);

  useEffect(() => {
    const seen = localStorage.getItem(STORAGE_KEY);
    if (!seen) {
      setIsOpen(true);
      setHasSeenOnboarding(false);
    }
  }, []);

  const openWizard = () => setIsOpen(true);
  const closeWizard = () => {
    setIsOpen(false);
    setHasSeenOnboarding(true);
  };

  return { isOpen, openWizard, closeWizard, hasSeenOnboarding };
}
