import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { Play, Loader2, CheckCircle, ArrowRight, Shuffle } from 'lucide-react';
import * as api from '../services/api';
import { DEMO_SCENARIOS } from '../types';

interface ScenarioResult {
  scenario: string;
  description: string;
  intents_created: number;
  intent_ids: string[];
}

function ScenarioCard({
  scenario,
  onRun,
  isRunning,
  result,
}: {
  scenario: (typeof DEMO_SCENARIOS)[number];
  onRun: () => void;
  isRunning: boolean;
  result?: ScenarioResult;
}) {
  const iconMap: Record<string, string> = {
    simple_swap: '🔄',
    intent_matching: '🎯',
    multi_hop: '🌐',
    cex_backstop: '🏦',
    auction_competition: '🏆',
  };

  return (
    <div className="card hover:border-cosmos-500/50 transition-colors">
      <div className="flex items-start gap-4">
        <div className="w-12 h-12 rounded-lg bg-cosmos-900/50 flex items-center justify-center text-2xl">
          {iconMap[scenario.id] || '📦'}
        </div>
        <div className="flex-1">
          <h3 className="font-semibold text-white">{scenario.name}</h3>
          <p className="text-sm text-gray-400 mt-1">{scenario.description}</p>

          {result && (
            <div className="mt-3 p-3 bg-green-900/20 rounded-lg border border-green-700/50">
              <div className="flex items-center gap-2 text-green-400 text-sm">
                <CheckCircle className="w-4 h-4" />
                <span>{result.intents_created} intent(s) created</span>
              </div>
            </div>
          )}
        </div>
        <button
          onClick={onRun}
          disabled={isRunning}
          className="btn-primary flex items-center gap-2 disabled:opacity-50"
        >
          {isRunning ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" />
              Running...
            </>
          ) : (
            <>
              <Play className="w-4 h-4" />
              Run
            </>
          )}
        </button>
      </div>
    </div>
  );
}

export default function DemoScenarios() {
  const [results, setResults] = useState<Record<string, ScenarioResult>>({});

  const scenarioMutation = useMutation({
    mutationFn: (name: string) => api.runScenario(name),
    onSuccess: (data) => {
      setResults((prev) => ({ ...prev, [data.scenario]: data }));
    },
  });

  const generateMutation = useMutation({
    mutationFn: api.generateDemoIntent,
  });

  return (
    <div className="space-y-6 animate-slide-in">
      <div>
        <h2 className="text-2xl font-bold text-white">Demo Scenarios</h2>
        <p className="text-gray-400">Run pre-configured scenarios to see the system in action</p>
      </div>

      {/* Quick Actions */}
      <div className="card">
        <h3 className="font-semibold text-white mb-4">Quick Actions</h3>
        <div className="flex gap-4">
          <button
            onClick={() => generateMutation.mutate()}
            disabled={generateMutation.isPending}
            className="btn-secondary flex items-center gap-2"
          >
            {generateMutation.isPending ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Shuffle className="w-4 h-4" />
            )}
            Generate Random Intent
          </button>
        </div>
        {generateMutation.isSuccess && (
          <div className="mt-3 p-3 bg-gray-800/50 rounded-lg">
            <p className="text-sm text-gray-300">{generateMutation.data.description}</p>
          </div>
        )}
      </div>

      {/* Scenarios */}
      <div className="space-y-4">
        <h3 className="font-semibold text-white">Available Scenarios</h3>
        {DEMO_SCENARIOS.map((scenario) => (
          <ScenarioCard
            key={scenario.id}
            scenario={scenario}
            onRun={() => scenarioMutation.mutate(scenario.id)}
            isRunning={
              scenarioMutation.isPending &&
              scenarioMutation.variables === scenario.id
            }
            result={results[scenario.id]}
          />
        ))}
      </div>

      {/* Scenario Details */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <div className="card">
          <h3 className="font-semibold text-white mb-4">Simple Swap Flow</h3>
          <div className="space-y-3">
            {[
              { step: 1, text: 'User submits ATOM → OSMO intent' },
              { step: 2, text: 'Intent broadcast to all solvers' },
              { step: 3, text: 'DEX Router queries Osmosis pools' },
              { step: 4, text: 'Batch auction runs (500ms)' },
              { step: 5, text: 'Best quote wins, settlement starts' },
              { step: 6, text: 'IBC transfer (2-5 seconds)' },
              { step: 7, text: 'User receives OSMO' },
            ].map(({ step, text }) => (
              <div key={step} className="flex items-center gap-3">
                <div className="w-6 h-6 rounded-full bg-cosmos-600 flex items-center justify-center text-xs text-white">
                  {step}
                </div>
                <span className="text-sm text-gray-300">{text}</span>
              </div>
            ))}
          </div>
        </div>

        <div className="card">
          <h3 className="font-semibold text-white mb-4">Intent Matching Flow</h3>
          <div className="space-y-3">
            <div className="p-4 bg-gray-800/50 rounded-lg">
              <div className="flex items-center justify-between">
                <div className="text-center">
                  <p className="text-sm text-gray-400">Alice</p>
                  <p className="text-white">50 ATOM</p>
                </div>
                <ArrowRight className="text-cosmos-400" />
                <div className="text-center">
                  <p className="text-sm text-gray-400">wants</p>
                  <p className="text-white">OSMO</p>
                </div>
              </div>
            </div>
            <div className="p-4 bg-gray-800/50 rounded-lg">
              <div className="flex items-center justify-between">
                <div className="text-center">
                  <p className="text-sm text-gray-400">Bob</p>
                  <p className="text-white">73 OSMO</p>
                </div>
                <ArrowRight className="text-cosmos-400" />
                <div className="text-center">
                  <p className="text-sm text-gray-400">wants</p>
                  <p className="text-white">ATOM</p>
                </div>
              </div>
            </div>
            <div className="p-4 bg-green-900/20 border border-green-700/50 rounded-lg">
              <p className="text-green-400 text-sm text-center">
                Direct match! Zero capital required, best prices for both.
              </p>
            </div>
          </div>
        </div>
      </div>

      {/* Architecture Diagram */}
      <div className="card bg-gradient-to-br from-cosmos-900/30 to-transparent border-cosmos-700/50">
        <h3 className="font-semibold text-white mb-4">System Architecture</h3>
        <pre className="text-xs text-gray-400 overflow-x-auto">
{`
┌─────────────────────────────────────────────────────────────────┐
│                         WEB INTERFACE                            │
│  Intent Creator │ Auction View │ Solver Dashboard │ Settlements  │
└────────────────────────────────┬────────────────────────────────┘
                                 │ REST/WebSocket
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                     SKIP SELECT (Coordination)                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
│  │ REST API │  │ WebSocket│  │ Matching │  │  Batch Auction │  │
│  └──────────┘  └──────────┘  └──────────┘  └────────────────┘  │
└────────────────────────────────┬────────────────────────────────┘
                                 │
        ┌────────────────────────┼────────────────────────┐
        ▼                        ▼                        ▼
┌───────────────┐      ┌───────────────┐      ┌───────────────┐
│  DEX Router   │      │    Intent     │      │    CEX        │
│    Solver     │      │    Matcher    │      │   Backstop    │
└───────────────┘      └───────────────┘      └───────────────┘
        │                        │                        │
        └────────────────────────┼────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                  SETTLEMENT (Two-Phase Commit)                   │
│  Escrow Contract │ Solver Registry │ IBC Relayer │ Execution    │
└─────────────────────────────────────────────────────────────────┘
`}
        </pre>
      </div>
    </div>
  );
}
