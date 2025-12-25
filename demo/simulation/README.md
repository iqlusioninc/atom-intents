# ATOM Intents Simulation Framework

Tools for simulating, testing, and benchmarking the intent-based liquidity system.

## Components

### Intent Generator (`intent_generator.py`)
Generate realistic trading intents for testing and demonstration.

### Market Simulator (`market_simulator.py`)
Simulate market conditions including price movements, liquidity, and volatility.

### Load Tester (`load_tester.py`)
Stress test the system with configurable load patterns.

### Analytics (`analytics.py`)
Collect and analyze performance metrics.

## Usage

### Generate Random Intents
```bash
python intent_generator.py --count 100 --output intents.json
```

### Run Market Simulation
```bash
python market_simulator.py --duration 3600 --volatility 0.02
```

### Load Test
```bash
python load_tester.py --target http://localhost:8080 --rps 50 --duration 300
```

## Configuration

See `config.yaml` for simulation parameters.
