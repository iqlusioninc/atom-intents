# ATOM Intents Testnet Integration

Scripts for deploying and testing on Cosmos testnet chains.

## Supported Testnets

| Chain | Testnet ID | RPC | Faucet |
|-------|------------|-----|--------|
| Cosmos Hub | theta-testnet-001 | https://rpc.sentry-01.theta-testnet.polypore.xyz | https://faucet.polypore.xyz |
| Osmosis | osmo-test-5 | https://rpc.testnet.osmosis.zone | https://faucet.testnet.osmosis.zone |
| Neutron | pion-1 | https://rpc-palvus.pion-1.ntrn.tech | https://faucet.pion-1.ntrn.tech |

## Prerequisites

- Go 1.21+
- Rust toolchain
- Docker
- jq
- curl

## Quick Start

### 1. Install CLI tools
```bash
./install-cli.sh
```

### 2. Configure accounts
```bash
./setup-accounts.sh --chain theta-testnet-001
```

### 3. Request testnet tokens
```bash
./request-tokens.sh --chain theta-testnet-001 --address cosmos1...
```

### 4. Deploy contracts
```bash
./deploy-contracts.sh --chain theta-testnet-001
```

### 5. Run demo
```bash
./run-demo.sh --chain theta-testnet-001
```

## Configuration

Copy `testnet.env.example` to `testnet.env` and fill in your values.

## Contract Deployment

The demo deploys the following contracts to testnet:
- Settlement Contract - Manages solver registration and settlement tracking
- Escrow Contract - Handles user fund lockup during settlement

## Testing on Testnet

After deployment, you can:
1. Access the web UI at the configured URL
2. Submit intents against real testnet chains
3. Watch settlements execute via IBC
