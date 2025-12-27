# Operations Guide: Deployments and Upgrades

This guide provides step-by-step procedures for deploying and upgrading the atom-intents system without failing inflight intents.

## Table of Contents

1. [System Overview](#system-overview)
2. [Backend Service Upgrades](#backend-service-upgrades)
3. [Smart Contract Migrations](#smart-contract-migrations)
4. [Monitoring During Upgrades](#monitoring-during-upgrades)
5. [Rollback Procedures](#rollback-procedures)
6. [Troubleshooting](#troubleshooting)

---

## System Overview

### Components

| Component | Type | Upgrade Frequency | Risk Level |
|-----------|------|-------------------|------------|
| Orchestrator | Backend service | Frequent | Low |
| Relayer | Backend service | Frequent | Low |
| Settlement Service | Backend service | Frequent | Low |
| Settlement Contract | CosmWasm | Rare (governance) | High |
| Escrow Contract | CosmWasm | Rare (governance) | High |

### Key Concepts

- **Drain Mode**: Stop accepting new intents while completing existing ones
- **Inflight Tracker**: Monitors all active settlements
- **Graceful Shutdown**: Wait for inflight intents before stopping
- **Contract Migration**: On-chain state preservation during upgrades

---

## Backend Service Upgrades

### Prerequisites

```bash
# Verify current service health
curl http://localhost:8080/health

# Check inflight count
curl http://localhost:8080/admin/inflight | jq '.count'
```

### Procedure: Rolling Restart (Recommended)

For zero-downtime upgrades with multiple replicas:

```bash
# 1. Deploy new version to one replica
kubectl set image deployment/orchestrator orchestrator=new-image:v2.0.0

# 2. Wait for new pod to be ready
kubectl rollout status deployment/orchestrator

# 3. Drain old pods one at a time (handled automatically by k8s)
```

### Procedure: Single Instance Graceful Shutdown

For single-instance deployments:

**Step 1: Start Drain Mode**

```bash
# Via HTTP API
curl -X POST http://localhost:8080/admin/upgrade/start \
  -H "Content-Type: application/json" \
  -d '{
    "reason": "Upgrading to v2.0.0",
    "drain_timeout_secs": 1800
  }'
```

Or programmatically in Rust:

```rust
use atom_intents_orchestrator::{DrainModeManager, GracefulShutdown};
use std::time::Duration;

// Get references from orchestrator
let drain_manager = orchestrator.drain_manager();
let inflight_tracker = orchestrator.inflight_tracker();

// Start drain
drain_manager.start_drain("Upgrading to v2.0.0".to_string(), 1800).await?;

// New intents will now receive 503 Service Unavailable
```

**Step 2: Monitor Drain Progress**

```bash
# Poll drain status
watch -n 5 'curl -s http://localhost:8080/admin/drain/status | jq'

# Expected output during drain:
# {
#   "mode": "Draining",
#   "inflight_count": 15,
#   "oldest_inflight_age_secs": 45,
#   "completed_since_drain": 23
# }
```

**Step 3: Wait for Completion**

```rust
use std::time::Duration;

// Wait up to 30 minutes for drain to complete
let result = drain_manager.wait_for_drain(Duration::from_secs(1800)).await?;

match result {
    DrainResult::Completed { elapsed, completed_count } => {
        info!("Drain completed in {:?}, {} intents completed", elapsed, completed_count);
        // Safe to shut down
    }
    DrainResult::TimedOut { remaining_intents, .. } => {
        warn!("{} intents still inflight after timeout", remaining_intents.len());
        // Check if any have locked funds
        let critical = remaining_intents.iter()
            .filter(|i| i.phase.has_locked_funds())
            .count();
        if critical > 0 {
            error!("{} intents have locked funds - DO NOT force shutdown", critical);
        }
    }
}
```

**Step 4: Stop Service**

```bash
# Only after drain completes or times out with no critical intents
systemctl stop atom-intents-orchestrator
```

**Step 5: Deploy New Version**

```bash
# Update binary
cp /path/to/new/orchestrator /usr/local/bin/orchestrator

# Run database migrations if needed
orchestrator migrate --database-url $DATABASE_URL

# Start service
systemctl start atom-intents-orchestrator

# Verify health
curl http://localhost:8080/health
```

**Step 6: Resume Operations**

```bash
# Resume accepting intents (automatic on restart, or manually)
curl -X POST http://localhost:8080/admin/drain/resume
```

---

## Smart Contract Migrations

### Prerequisites

- Governance proposal approved (for mainnet)
- New WASM code uploaded and code_id obtained
- Backend services in drain mode

### Procedure: Contract Migration

**Step 1: Drain Backend**

```bash
# Stop backend from creating new settlements
curl -X POST http://localhost:8080/admin/upgrade/start \
  -d '{"reason": "Contract migration", "drain_timeout_secs": 3600}'

# Wait for drain
curl http://localhost:8080/admin/drain/status
```

**Step 2: Query Inflight Settlements On-Chain**

```bash
# Query inflight settlements from contract
INFLIGHT=$(wasmd query wasm contract-state smart $CONTRACT_ADDR \
  '{"inflight_settlements": {"limit": 100}}' \
  --output json | jq '.data.count')

echo "Inflight settlements: $INFLIGHT"
```

**Step 3: Execute Migration**

```bash
# Prepare migration message
MIGRATE_MSG='{
  "new_version": "2.0.0",
  "config": {
    "preserve_inflight": true,
    "stuck_settlement_action": {
      "extend_timeout": {
        "additional_seconds": 3600
      }
    },
    "extend_timeout_secs": 7200,
    "new_config": null
  }
}'

# Execute migration (requires admin/governance)
wasmd tx wasm migrate $CONTRACT_ADDR $NEW_CODE_ID "$MIGRATE_MSG" \
  --from admin \
  --gas auto \
  --gas-adjustment 1.3 \
  -y
```

**Step 4: Verify Migration**

```bash
# Query migration info
wasmd query wasm contract-state smart $CONTRACT_ADDR \
  '{"migration_info": {}}' \
  --output json | jq

# Expected output:
# {
#   "data": {
#     "previous_version": "1.0.0",
#     "current_version": "2.0.0",
#     "migrated_at": 1703123456,
#     "preserved_inflight_count": 5
#   }
# }

# Verify inflight settlements preserved
wasmd query wasm contract-state smart $CONTRACT_ADDR \
  '{"inflight_settlements": {}}' \
  --output json | jq '.data.count'
```

**Step 5: Update Backend Configuration**

```bash
# Update config to point to migrated contract (if address changed)
# Usually not needed as migrate keeps same address

# Restart backend with new contract version support
systemctl restart atom-intents-orchestrator
```

**Step 6: Resume Operations**

```bash
# Backend auto-resumes on restart, or manually:
curl -X POST http://localhost:8080/admin/drain/resume
```

### Migration Message Options

```rust
// Preserve all inflight, extend timeouts by 2 hours
MigrateMsg {
    new_version: "2.0.0".to_string(),
    config: Some(MigrationConfig {
        preserve_inflight: true,
        stuck_settlement_action: StuckSettlementAction::ExtendTimeout {
            additional_seconds: 7200,
        },
        extend_timeout_secs: Some(7200),
        new_config: None,
    }),
}

// Fail stuck settlements, update config
MigrateMsg {
    new_version: "2.0.0".to_string(),
    config: Some(MigrationConfig {
        preserve_inflight: true,
        stuck_settlement_action: StuckSettlementAction::RefundAndFail,
        extend_timeout_secs: None,
        new_config: Some(ConfigUpdate {
            admin: Some("cosmos1newadmin...".to_string()),
            escrow_contract: None,
            min_solver_bond: Some(Uint128::new(5_000_000)),
            base_slash_bps: Some(300),
        }),
    }),
}

// Simple migration, preserve everything as-is
MigrateMsg {
    new_version: "2.0.0".to_string(),
    config: None, // Defaults: preserve_inflight=true, stuck_action=Preserve
}
```

---

## Monitoring During Upgrades

### Key Metrics

```bash
# Prometheus queries for upgrade monitoring

# Inflight count
atom_intents_inflight_count

# Drain mode state (0=active, 1=draining, 2=drained)
atom_intents_drain_mode

# Settlements completed since drain started
atom_intents_drain_completed_total

# Time since drain started
time() - atom_intents_drain_start_timestamp
```

### Alerts

```yaml
# Alert if drain takes too long
- alert: UpgradeDrainTimeout
  expr: atom_intents_drain_mode == 1 and (time() - atom_intents_drain_start_timestamp) > 1800
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Upgrade drain taking longer than expected"

# Alert if inflight count not decreasing
- alert: UpgradeInflightStuck
  expr: atom_intents_drain_mode == 1 and delta(atom_intents_inflight_count[10m]) >= 0
  for: 10m
  labels:
    severity: critical
  annotations:
    summary: "Inflight settlements not completing during drain"
```

### Log Queries

```bash
# Follow drain progress in logs
journalctl -u atom-intents-orchestrator -f | grep -E "(drain|inflight|upgrade)"

# Key log messages to watch for:
# "Starting drain mode"
# "Drain in progress... remaining=X"
# "Drain completed - all inflight intents finished"
# "Rejecting intent - system is draining"
```

---

## Rollback Procedures

### Backend Rollback

```bash
# Stop new version
systemctl stop atom-intents-orchestrator

# Restore previous binary
cp /backup/orchestrator-v1.0.0 /usr/local/bin/orchestrator

# Rollback database migrations if needed
orchestrator migrate --database-url $DATABASE_URL --target-version 2

# Start previous version
systemctl start atom-intents-orchestrator

# Resume operations
curl -X POST http://localhost:8080/admin/drain/resume
```

### Contract Rollback

Contract rollback requires another governance proposal to migrate to the previous code.

```bash
# Upload previous WASM (if not already on chain)
wasmd tx wasm store /path/to/old_contract.wasm --from admin

# Get the code_id from tx result
OLD_CODE_ID=<from tx result>

# Execute rollback migration
ROLLBACK_MSG='{
  "new_version": "1.0.0",
  "config": {
    "preserve_inflight": true,
    "stuck_settlement_action": "preserve"
  }
}'

wasmd tx wasm migrate $CONTRACT_ADDR $OLD_CODE_ID "$ROLLBACK_MSG" \
  --from admin \
  --gas auto \
  -y
```

---

## Troubleshooting

### Problem: Drain Not Completing

**Symptoms:**
- Inflight count not reaching zero
- Settlements stuck in intermediate states

**Diagnosis:**
```bash
# Check stuck settlements
curl http://localhost:8080/admin/inflight | jq '.intents[] | select(.phase | contains("Locked"))'

# Check for IBC packets in flight
curl http://localhost:8080/admin/ibc/pending
```

**Solutions:**

1. **IBC packets stuck**: Wait for IBC timeout (usually 10-30 minutes)
   ```bash
   # Check oldest inflight age
   curl http://localhost:8080/admin/drain/status | jq '.oldest_inflight_age_secs'
   ```

2. **Settlement in recovery**: Wait for recovery to complete
   ```bash
   # Check recovery status
   curl http://localhost:8080/admin/recovery/status
   ```

3. **Force complete after investigation**: Only if no user funds at risk
   ```bash
   # Force drain (DANGEROUS - only if no locked funds)
   curl -X POST http://localhost:8080/admin/drain/force
   ```

### Problem: Migration Rejected - Inflight Exists

**Symptoms:**
```
Error: Inflight settlements exist: 5 settlements must complete before migration
```

**Solutions:**

1. Wait for settlements to complete
2. Use `preserve_inflight: true` in migration config
3. Check if backend drain was not started

### Problem: Settlements Expired During Upgrade

**Symptoms:**
- Settlements marked as failed after migration
- Users reporting refunds

**Prevention:**
- Always use `extend_timeout_secs` in migration config
- Plan upgrades during low-traffic periods

**Resolution:**
- Review failed settlements
- Process refunds if needed
- Communicate with affected users

### Problem: Backend Can't Connect After Contract Migration

**Symptoms:**
- Backend errors querying contract
- "Invalid message" errors

**Solutions:**

1. Verify contract address didn't change
2. Update backend to support new message formats
3. Check contract query compatibility

```bash
# Test contract queries manually
wasmd query wasm contract-state smart $CONTRACT_ADDR '{"config": {}}'
```

---

## Quick Reference

### Drain Mode States

| State | Description | New Intents | Inflight Processing |
|-------|-------------|-------------|---------------------|
| Active | Normal operation | Accepted | Yes |
| Draining | Shutdown initiated | Rejected (503) | Yes |
| Drained | All complete | Rejected | No |
| Upgrading | Upgrade in progress | Rejected | No |

### API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/admin/drain/start` | POST | Start drain mode |
| `/admin/drain/status` | GET | Get drain status |
| `/admin/drain/cancel` | POST | Cancel drain, resume |
| `/admin/drain/force` | POST | Force drain (dangerous) |
| `/admin/inflight` | GET | List inflight intents |
| `/admin/upgrade/start` | POST | Start upgrade process |

### Contract Queries

```json
// Get migration info
{"migration_info": {}}

// Get inflight settlements
{"inflight_settlements": {"start_after": null, "limit": 100}}

// Get config
{"config": {}}
```

### Safety Checklist

Before any upgrade:
- [ ] Reviewed changes for breaking modifications
- [ ] Tested upgrade procedure in staging
- [ ] Verified rollback procedure works
- [ ] Scheduled during low-traffic window
- [ ] Notified team/stakeholders
- [ ] Backup of current state taken

During upgrade:
- [ ] Drain mode activated
- [ ] Monitoring dashboards open
- [ ] Team available for support
- [ ] Rollback plan ready

After upgrade:
- [ ] Health checks passing
- [ ] Metrics normal
- [ ] Test transactions successful
- [ ] Resume mode confirmed
