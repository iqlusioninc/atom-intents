# ATOM Intents Testnet Deployment

Deployed: 2024-12-28

## Deployer Wallet

| Chain | Address |
|-------|---------|
| Cosmos Hub (provider) | `cosmos1q6u2k563ep39un0p0j0p6ked5653dr0yrmmu9d` |
| Osmosis (osmo-test-5) | `osmo1q6u2k563ep39un0p0j0p6ked5653dr0ytqgvnl` |
| Neutron (pion-1) | `neutron1q6u2k563ep39un0p0j0p6ked5653dr0y8yj7l2` |

Mnemonic stored in: `~/.atom-intents/testnet-wallet.txt`

---

## Cosmos Hub Provider Testnet

**Chain ID:** `provider`
**RPC:** `https://cosmos-testnet-rpc.polkachu.com:443`
**gRPC:** `https://cosmos-testnet-grpc.polkachu.com:443`

### Contracts

| Contract | Code ID | Address |
|----------|---------|---------|
| Settlement | 168 | `cosmos1xwft7w6kcspzufftw6ky4f5e8sykumpuenpm34tkxk4epmya0jdsahgsff` |
| Escrow | 169 | `cosmos13jv2umdqvlkfncpd6vf7r2sc0ljdtenmzujlpqqpgagarassqsws86phq9` |

### Configuration

- **Admin:** `cosmos1q6u2k563ep39un0p0j0p6ked5653dr0yrmmu9d`
- **Min Solver Bond:** 1,000,000 uatom (1 ATOM)
- **Base Slash BPS:** 500 (5%)

---

## Neutron Testnet (Pion-1)

**Chain ID:** `pion-1`
**RPC:** `https://neutron-testnet-rpc.polkachu.com:443`

### Contracts

| Contract | Code ID | Address |
|----------|---------|---------|
| Settlement | 13671 | `neutron17c6wr9krx2m4earvyyn8p428tyrpk9dka30axh2drfv3udw7wqgsh4ewfu` |
| Escrow | 13672 | `neutron1vytx7tejzyejjurawm8jltvtq9qrwyu73w2fxmchqgs057d9xrfqm3t4j6` |

### Configuration

- **Admin:** `neutron1q6u2k563ep39un0p0j0p6ked5653dr0y8yj7l2`
- **Min Solver Bond:** 1,000,000 untrn (1 NTRN)
- **Base Slash BPS:** 500 (5%)

---

## Osmosis Testnet

**Chain ID:** `osmo-test-5`
**RPC:** `https://rpc.testnet.osmosis.zone:443`

**Status:** Not deployed (optional)

> **Note:** Osmosis deployment is deferred. The demo system works without it - the
> skip-select-simulator uses mock solvers for liquidity. Osmosis can be added later
> for real DEX routing and cross-chain swap testing.
>
> To deploy when ready:
> 1. Request tokens from https://faucet.testnet.osmosis.zone/
> 2. Send to: `osmo1q6u2k563ep39un0p0j0p6ked5653dr0ytqgvnl`
> 3. Run deployment script with `--chain osmo-test-5`

---

## Running the Demo

```bash
# Start demo with real testnet contracts
cd demo/skip-select-simulator
./scripts/start-testnet.sh
```

## Verification Commands

```bash
# Cosmos Hub
gaiad query wasm contract-state smart \
  cosmos1xwft7w6kcspzufftw6ky4f5e8sykumpuenpm34tkxk4epmya0jdsahgsff \
  '{"config":{}}' \
  --node https://cosmos-testnet-rpc.polkachu.com:443

# Neutron
neutrond query wasm contract-state smart \
  neutron17c6wr9krx2m4earvyyn8p428tyrpk9dka30axh2drfv3udw7wqgsh4ewfu \
  '{"config":{}}' \
  --node https://neutron-testnet-rpc.polkachu.com:443
```
