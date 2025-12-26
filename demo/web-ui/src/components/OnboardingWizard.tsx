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
        <div className="grid grid-cols-2 gap-4 mt-4">
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
        <div className="relative py-6">
          <div className="flex items-center justify-between">
            <div className="text-center">
              <div className="w-16 h-16 rounded-full bg-cosmos-600 flex items-center justify-center mx-auto mb-2">
                <span className="text-2xl">👤</span>
              </div>
              <p className="text-xs text-gray-400">Your Intent</p>
            </div>
            <div className="flex-1 flex items-center justify-center">
              <ArrowRightLeft className="w-6 h-6 text-gray-500" />
            </div>
            <div className="text-center">
              <div className="w-16 h-16 rounded-full bg-yellow-600 flex items-center justify-center mx-auto mb-2">
                <span className="text-2xl">🏆</span>
              </div>
              <p className="text-xs text-gray-400">Batch Auction</p>
            </div>
            <div className="flex-1 flex items-center justify-center">
              <ArrowRightLeft className="w-6 h-6 text-gray-500" />
            </div>
            <div className="text-center">
              <div className="w-16 h-16 rounded-full bg-green-600 flex items-center justify-center mx-auto mb-2">
                <span className="text-2xl">✓</span>
              </div>
              <p className="text-xs text-gray-400">Best Price Wins</p>
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
        <div className="grid grid-cols-2 gap-3 mt-4">
          <div className="p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-sm">Create Intent</p>
            <p className="text-xs text-gray-400">Submit a swap intent and watch it get filled</p>
          </div>
          <div className="p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-sm">Watch Auctions</p>
            <p className="text-xs text-gray-400">See solvers compete in real-time</p>
          </div>
          <div className="p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-sm">Track Settlements</p>
            <p className="text-xs text-gray-400">Follow the escrow and IBC flow</p>
          </div>
          <div className="p-3 bg-gray-800/50 rounded-lg">
            <p className="text-cosmos-400 font-medium text-sm">Run Scenarios</p>
            <p className="text-xs text-gray-400">Try pre-built demo flows</p>
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
    <div className="fixed inset-0 z-[100] flex items-center justify-center">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/80 backdrop-blur-sm"
        onClick={handleClose}
      />

      {/* Modal */}
      <div className="relative w-full max-w-2xl mx-4 bg-gray-900 rounded-2xl border border-gray-700 shadow-2xl overflow-hidden">
        {/* Close button */}
        <button
          onClick={handleClose}
          className="absolute top-4 right-4 p-2 text-gray-400 hover:text-white transition-colors z-10"
        >
          <X className="w-5 h-5" />
        </button>

        {/* Content */}
        <div className="p-8">
          {/* Icon */}
          <div className="flex justify-center mb-6">
            {slide.icon}
          </div>

          {/* Title */}
          <h2 className="text-2xl font-bold text-white text-center mb-2">
            {slide.title}
          </h2>
          {slide.subtitle && (
            <p className="text-gray-400 text-center mb-6">
              {slide.subtitle}
            </p>
          )}

          {/* Content */}
          <div className="text-gray-300 min-h-[250px]">
            {slide.content}
          </div>
        </div>

        {/* Footer */}
        <div className="px-8 pb-6">
          {/* Progress dots */}
          <div className="flex justify-center gap-2 mb-6">
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
          <div className="flex justify-between items-center">
            <button
              onClick={prevSlide}
              disabled={currentSlide === 0}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg transition-colors ${
                currentSlide === 0
                  ? 'text-gray-600 cursor-not-allowed'
                  : 'text-gray-400 hover:text-white hover:bg-gray-800'
              }`}
            >
              <ChevronLeft className="w-4 h-4" />
              Back
            </button>

            <button
              onClick={nextSlide}
              className="flex items-center gap-2 px-6 py-2 bg-cosmos-600 hover:bg-cosmos-500 text-white rounded-lg transition-colors"
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
