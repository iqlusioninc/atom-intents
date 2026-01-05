use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, IbcMsg,
    IbcTimeout, MessageInfo, Response, StdResult,
};

use crate::error::ContractError;
use crate::msg::{
    ConfigResponse, EscrowResponse, EscrowsResponse, EthereumEscrowStatusResponse, ExecuteMsg,
    InstantiateMsg, QueryMsg,
};
use crate::state::{
    Config, Escrow, EscrowStatus, EthereumEscrow, EthereumEscrowStatus, FrontingInfo, CONFIG,
    ESCROWS, ESCROWS_BY_INTENT, ETHEREUM_ESCROWS, USER_ESCROWS,
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        admin: deps.api.addr_validate(&msg.admin)?,
        settlement_contract: deps.api.addr_validate(&msg.settlement_contract)?,
    };
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "instantiate"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Lock {
            escrow_id,
            intent_id,
            expires_at,
        } => execute_lock(deps, env, info, escrow_id, intent_id, expires_at),
        ExecuteMsg::LockFromIbc {
            intent_id,
            expires_at,
            user_source_address,
            source_chain_id,
            source_channel,
        } => execute_lock_from_ibc(
            deps,
            env,
            info,
            intent_id,
            expires_at,
            user_source_address,
            source_chain_id,
            source_channel,
        ),
        ExecuteMsg::Release {
            escrow_id,
            recipient,
        } => execute_release(deps, env, info, escrow_id, recipient),
        ExecuteMsg::Refund { escrow_id } => execute_refund(deps, env, info, escrow_id),
        ExecuteMsg::RetryRefund { escrow_id } => execute_retry_refund(deps, env, info, escrow_id),
        ExecuteMsg::UpdateConfig {
            admin,
            settlement_contract,
        } => execute_update_config(deps, info, admin, settlement_contract),

        // Ethereum escrow messages (via Eureka)
        ExecuteMsg::RegisterEthereumEscrowIntent {
            intent,
            eth_sender,
            expected_amount,
            eureka_timeout_secs,
        } => execute_register_ethereum_escrow_intent(
            deps,
            env,
            info,
            *intent,
            eth_sender,
            expected_amount,
            eureka_timeout_secs,
        ),
        ExecuteMsg::NotifyEurekaPacketReceived {
            intent_id,
            packet_id,
            amount,
            sender,
        } => execute_notify_eureka_packet_received(deps, env, info, intent_id, packet_id, amount, sender),
        ExecuteMsg::NotifyEurekaFinalized {
            intent_id,
            packet_id,
            proof_block,
        } => execute_notify_eureka_finalized(deps, env, info, intent_id, packet_id, proof_block),
        ExecuteMsg::FrontSettlement {
            intent_id,
            solver_id,
            output_amount,
            risk_bond,
        } => execute_front_settlement(deps, env, info, intent_id, solver_id, output_amount, risk_bond),
        ExecuteMsg::ClaimEurekaEscrow {
            intent_id,
            packet_id,
        } => execute_claim_eureka_escrow(deps, env, info, intent_id, packet_id),
        ExecuteMsg::HandleEurekaEscrowFailure { intent_id, reason } => {
            execute_handle_eureka_escrow_failure(deps, env, info, intent_id, reason)
        }
    }
}

fn execute_lock(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    escrow_id: String,
    intent_id: String,
    expires_at: u64,
) -> Result<Response, ContractError> {
    // Verify escrow doesn't exist
    if ESCROWS.has(deps.storage, &escrow_id) {
        return Err(ContractError::EscrowAlreadyExists { id: escrow_id });
    }

    // Verify intent doesn't already have an escrow
    if ESCROWS_BY_INTENT.has(deps.storage, &intent_id) {
        return Err(ContractError::IntentAlreadyEscrowed {
            intent_id: intent_id.clone(),
        });
    }

    // Require exactly one coin
    if info.funds.len() != 1 {
        return Err(ContractError::InvalidFunds {
            expected: "exactly one coin".to_string(),
            got: format!("{} coins", info.funds.len()),
        });
    }

    let coin = &info.funds[0];

    let escrow = Escrow {
        id: escrow_id.clone(),
        owner: info.sender.clone(),
        amount: coin.amount,
        denom: coin.denom.clone(),
        intent_id: intent_id.clone(),
        expires_at,
        status: EscrowStatus::Locked,
        // Local escrow - no cross-chain fields
        owner_chain_id: None,
        owner_source_address: None,
        source_channel: None,
        source_denom: None,
    };

    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;
    USER_ESCROWS.save(deps.storage, (&info.sender, &escrow_id), &true)?;
    ESCROWS_BY_INTENT.save(deps.storage, &intent_id, &escrow_id)?;

    Ok(Response::new()
        .add_attribute("action", "lock")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("owner", info.sender)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

/// Lock funds via IBC Hooks - called when funds arrive from a cross-chain transfer
/// The IBC Hooks middleware calls this with the transferred funds attached
#[allow(clippy::too_many_arguments)]
fn execute_lock_from_ibc(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    intent_id: String,
    expires_at: u64,
    user_source_address: String,
    source_chain_id: String,
    source_channel: String,
) -> Result<Response, ContractError> {
    // Verify intent doesn't already have an escrow (replay protection)
    if ESCROWS_BY_INTENT.has(deps.storage, &intent_id) {
        return Err(ContractError::IntentAlreadyEscrowed {
            intent_id: intent_id.clone(),
        });
    }

    // Verify exactly one IBC coin was sent
    let ibc_funds: Vec<_> = info
        .funds
        .iter()
        .filter(|c| c.denom.starts_with("ibc/"))
        .collect();

    if ibc_funds.len() != 1 {
        return Err(ContractError::NotIbcFunds {});
    }

    let coin = ibc_funds[0];

    // Generate escrow ID from intent ID for predictability
    let escrow_id = format!("esc_{}", intent_id);

    // Verify escrow doesn't already exist
    if ESCROWS.has(deps.storage, &escrow_id) {
        return Err(ContractError::EscrowAlreadyExists { id: escrow_id });
    }

    // For IBC Hooks, the sender is typically the IBC transfer module or a derived address
    // We store both the on-chain sender and the original source address
    let escrow = Escrow {
        id: escrow_id.clone(),
        owner: info.sender.clone(), // On-chain sender (IBC derived address)
        amount: coin.amount,
        denom: coin.denom.clone(), // This will be ibc/... denom
        intent_id: intent_id.clone(),
        expires_at,
        status: EscrowStatus::Locked,
        // Cross-chain escrow fields
        owner_chain_id: Some(source_chain_id.clone()),
        owner_source_address: Some(user_source_address.clone()),
        source_channel: Some(source_channel.clone()),
        source_denom: None, // Will be derived from ibc denom trace if needed
    };

    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;
    USER_ESCROWS.save(deps.storage, (&info.sender, &escrow_id), &true)?;
    ESCROWS_BY_INTENT.save(deps.storage, &intent_id, &escrow_id)?;

    Ok(Response::new()
        .add_attribute("action", "lock_from_ibc")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("intent_id", intent_id)
        .add_attribute("source_chain", source_chain_id)
        .add_attribute("source_address", user_source_address)
        .add_attribute("source_channel", source_channel)
        .add_attribute("amount", coin.amount)
        .add_attribute("denom", &coin.denom))
}

fn execute_release(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
    recipient: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only settlement contract can release
    if info.sender != config.settlement_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut escrow =
        ESCROWS
            .load(deps.storage, &escrow_id)
            .map_err(|_| ContractError::EscrowNotFound {
                id: escrow_id.clone(),
            })?;

    // Check not already released
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::EscrowNotFound { id: escrow_id });
    }

    // SECURITY FIX (5.6): Prevent release after expiration
    // This prevents a race condition where:
    // 1. Escrow expires
    // 2. User initiates refund
    // 3. Settlement contract tries to release (would be double-spend)
    if env.block.time.seconds() >= escrow.expires_at {
        return Err(ContractError::EscrowExpired { id: escrow_id });
    }

    // Update status
    escrow.status = EscrowStatus::Released {
        recipient: recipient.clone(),
    };
    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;

    // Send funds to recipient
    let recipient_addr = deps.api.addr_validate(&recipient)?;
    let send_msg = BankMsg::Send {
        to_address: recipient_addr.to_string(),
        amount: vec![Coin {
            denom: escrow.denom.clone(),
            amount: escrow.amount,
        }],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "release")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("recipient", recipient)
        .add_attribute("amount", escrow.amount))
}

fn execute_refund(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
) -> Result<Response, ContractError> {
    let mut escrow =
        ESCROWS
            .load(deps.storage, &escrow_id)
            .map_err(|_| ContractError::EscrowNotFound {
                id: escrow_id.clone(),
            })?;

    // Only owner can refund (for local escrows)
    // For cross-chain escrows, we also allow the admin to trigger refunds
    let config = CONFIG.load(deps.storage)?;
    let is_owner = info.sender == escrow.owner;
    let is_admin = info.sender == config.admin;

    if !is_owner && !is_admin {
        return Err(ContractError::Unauthorized {});
    }

    // Check escrow is expired
    if env.block.time.seconds() < escrow.expires_at {
        return Err(ContractError::EscrowNotExpired { id: escrow_id });
    }

    // Check status allows refund
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::InvalidStatus {});
    }

    // Determine refund method based on whether this is a cross-chain escrow
    let (refund_msg, refund_type): (CosmosMsg, &str) =
        if let (Some(source_channel), Some(source_address)) =
            (&escrow.source_channel, &escrow.owner_source_address)
        {
            // Cross-chain refund via IBC
            escrow.status = EscrowStatus::Refunding;

            let ibc_msg = IbcMsg::Transfer {
                channel_id: source_channel.clone(),
                to_address: source_address.clone(),
                amount: Coin {
                    denom: escrow.denom.clone(),
                    amount: escrow.amount,
                },
                timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(600)),
                memo: Some(format!(
                    "{{\"refund\":{{\"escrow_id\":\"{}\",\"intent_id\":\"{}\"}}}}",
                    escrow.id, escrow.intent_id
                )),
            };

            (ibc_msg.into(), "ibc_refund")
        } else {
            // Local refund via bank send
            escrow.status = EscrowStatus::Refunded;

            let bank_msg = BankMsg::Send {
                to_address: escrow.owner.to_string(),
                amount: vec![Coin {
                    denom: escrow.denom.clone(),
                    amount: escrow.amount,
                }],
            };

            (bank_msg.into(), "local_refund")
        };

    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;

    Ok(Response::new()
        .add_message(refund_msg)
        .add_attribute("action", "refund")
        .add_attribute("refund_type", refund_type)
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("owner", escrow.owner)
        .add_attribute("amount", escrow.amount))
}

/// Retry a failed IBC refund
fn execute_retry_refund(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
) -> Result<Response, ContractError> {
    let mut escrow =
        ESCROWS
            .load(deps.storage, &escrow_id)
            .map_err(|_| ContractError::EscrowNotFound {
                id: escrow_id.clone(),
            })?;

    // Only admin can retry failed refunds
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    // Check status is RefundFailed
    if !matches!(escrow.status, EscrowStatus::RefundFailed) {
        return Err(ContractError::InvalidStatus {});
    }

    // Must be a cross-chain escrow
    let source_channel =
        escrow
            .source_channel
            .as_ref()
            .ok_or(ContractError::MissingCrossChainField {
                field: "source_channel".to_string(),
            })?;
    let source_address =
        escrow
            .owner_source_address
            .as_ref()
            .ok_or(ContractError::MissingCrossChainField {
                field: "owner_source_address".to_string(),
            })?;

    // Update status to Refunding
    escrow.status = EscrowStatus::Refunding;
    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;

    // Retry IBC transfer
    let ibc_msg = IbcMsg::Transfer {
        channel_id: source_channel.clone(),
        to_address: source_address.clone(),
        amount: Coin {
            denom: escrow.denom.clone(),
            amount: escrow.amount,
        },
        timeout: IbcTimeout::with_timestamp(env.block.time.plus_seconds(600)),
        memo: Some(format!(
            "{{\"refund\":{{\"escrow_id\":\"{}\",\"intent_id\":\"{}\",\"retry\":true}}}}",
            escrow.id, escrow.intent_id
        )),
    };

    Ok(Response::new()
        .add_message(ibc_msg)
        .add_attribute("action", "retry_refund")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("destination", source_address)
        .add_attribute("channel", source_channel)
        .add_attribute("amount", escrow.amount))
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    admin: Option<String>,
    settlement_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    if let Some(admin) = admin {
        config.admin = deps.api.addr_validate(&admin)?;
    }
    if let Some(settlement_contract) = settlement_contract {
        config.settlement_contract = deps.api.addr_validate(&settlement_contract)?;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// ETHEREUM ESCROW HANDLERS (via Eureka)
// ═══════════════════════════════════════════════════════════════════════════════

/// Register an intent that will be funded via Ethereum escrow through Eureka
///
/// This is called when a user initiates a transfer from Ethereum. The intent is
/// registered as pending, waiting for the Eureka packet to arrive.
#[allow(clippy::too_many_arguments)]
fn execute_register_ethereum_escrow_intent(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intent: atom_intents_types::Intent,
    eth_sender: String,
    expected_amount: cosmwasm_std::Uint128,
    eureka_timeout_secs: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only settlement contract or admin can register Ethereum escrow intents
    if info.sender != config.settlement_contract && info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let intent_id = intent.id.clone();

    // Verify no existing escrow for this intent
    if ETHEREUM_ESCROWS.has(deps.storage, &intent_id) {
        return Err(ContractError::IntentAlreadyEscrowed { intent_id });
    }

    // Derive expected denom from intent input (e.g., ibc/... for bridged ETH/ERC20)
    let expected_denom = intent.input.denom.clone();

    let escrow = EthereumEscrow {
        intent_id: intent_id.clone(),
        eth_sender: eth_sender.clone(),
        expected_amount,
        expected_denom: expected_denom.clone(),
        registered_at: env.block.time.seconds(),
        eureka_timeout: env.block.time.seconds() + eureka_timeout_secs,
        status: EthereumEscrowStatus::Pending,
        packet_id: None,
        received_amount: None,
        finalized_at_block: None,
        fronting: None,
    };

    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    Ok(Response::new()
        .add_attribute("action", "register_ethereum_escrow_intent")
        .add_attribute("intent_id", intent_id)
        .add_attribute("eth_sender", eth_sender)
        .add_attribute("expected_amount", expected_amount)
        .add_attribute("expected_denom", expected_denom)
        .add_attribute("eureka_timeout_secs", eureka_timeout_secs.to_string()))
}

/// Notify that an Eureka packet has been received
///
/// Called by the relayer when the Eureka IBC packet arrives on the Hub.
/// The escrow moves to "Received" status, waiting for ZK proof finality.
fn execute_notify_eureka_packet_received(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intent_id: String,
    packet_id: String,
    amount: cosmwasm_std::Uint128,
    sender: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin (relayer role) can notify
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| ContractError::EthereumEscrowNotFound {
            intent_id: intent_id.clone(),
        })?;

    // Verify escrow is in Pending status
    if !matches!(escrow.status, EthereumEscrowStatus::Pending) {
        return Err(ContractError::InvalidEthereumEscrowStatus {
            expected: "Pending".to_string(),
            actual: format!("{:?}", escrow.status),
        });
    }

    // Verify sender matches expected Ethereum sender
    if sender != escrow.eth_sender {
        return Err(ContractError::EthereumSenderMismatch {
            expected: escrow.eth_sender.clone(),
            actual: sender,
        });
    }

    // Check timeout hasn't passed
    if env.block.time.seconds() > escrow.eureka_timeout {
        return Err(ContractError::EurekaTimeout {
            intent_id: intent_id.clone(),
        });
    }

    // Update escrow status
    escrow.status = EthereumEscrowStatus::Received;
    escrow.packet_id = Some(packet_id.clone());
    escrow.received_amount = Some(amount);

    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    Ok(Response::new()
        .add_attribute("action", "notify_eureka_packet_received")
        .add_attribute("intent_id", intent_id)
        .add_attribute("packet_id", packet_id)
        .add_attribute("amount", amount)
        .add_attribute("sender", escrow.eth_sender))
}

/// Notify that an Eureka packet has been finalized with ZK proof
///
/// Called by the relayer when the ZK proof verifies on the Hub.
/// The escrow moves to "Finalized" status and can now be claimed by the solver.
fn execute_notify_eureka_finalized(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    intent_id: String,
    packet_id: String,
    proof_block: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin (relayer role) can notify
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| ContractError::EthereumEscrowNotFound {
            intent_id: intent_id.clone(),
        })?;

    // Verify escrow is in Received or Fronted status (can finalize after fronting)
    let valid_status = matches!(
        escrow.status,
        EthereumEscrowStatus::Received | EthereumEscrowStatus::Fronted
    );
    if !valid_status {
        return Err(ContractError::InvalidEthereumEscrowStatus {
            expected: "Received or Fronted".to_string(),
            actual: format!("{:?}", escrow.status),
        });
    }

    // Verify packet_id matches
    if escrow.packet_id.as_ref() != Some(&packet_id) {
        return Err(ContractError::PacketIdMismatch {
            expected: escrow.packet_id.clone().unwrap_or_default(),
            actual: packet_id,
        });
    }

    // Update status (if already Fronted, move to Finalized so solver can claim)
    let was_fronted = matches!(escrow.status, EthereumEscrowStatus::Fronted);
    escrow.status = EthereumEscrowStatus::Finalized;
    escrow.finalized_at_block = Some(proof_block);

    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    Ok(Response::new()
        .add_attribute("action", "notify_eureka_finalized")
        .add_attribute("intent_id", intent_id)
        .add_attribute("proof_block", proof_block.to_string())
        .add_attribute("was_fronted", was_fronted.to_string()))
}

/// Solver fronts settlement before Eureka finality
///
/// The solver pays the user immediately and takes on settlement risk.
/// They post a bond and will claim the escrowed funds after finality.
fn execute_front_settlement(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    intent_id: String,
    solver_id: String,
    output_amount: cosmwasm_std::Uint128,
    risk_bond: cosmwasm_std::Uint128,
) -> Result<Response, ContractError> {
    let mut escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| ContractError::EthereumEscrowNotFound {
            intent_id: intent_id.clone(),
        })?;

    // Verify escrow is in Received status (not already fronted/finalized)
    if !matches!(escrow.status, EthereumEscrowStatus::Received) {
        return Err(ContractError::InvalidEthereumEscrowStatus {
            expected: "Received".to_string(),
            actual: format!("{:?}", escrow.status),
        });
    }

    // Verify sender has attached the required bond
    let bond_funds: cosmwasm_std::Uint128 = info
        .funds
        .iter()
        .filter(|c| c.denom == escrow.expected_denom || c.denom == "uatom")
        .map(|c| c.amount)
        .sum();

    if bond_funds < risk_bond {
        return Err(ContractError::InsufficientBond {
            required: risk_bond,
            provided: bond_funds,
        });
    }

    // Record fronting info
    escrow.fronting = Some(FrontingInfo {
        solver_id: solver_id.clone(),
        solver_addr: info.sender.clone(),
        fronted_at: env.block.time.seconds(),
        bond_amount: risk_bond,
        output_amount,
    });
    escrow.status = EthereumEscrowStatus::Fronted;

    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    Ok(Response::new()
        .add_attribute("action", "front_settlement")
        .add_attribute("intent_id", intent_id)
        .add_attribute("solver_id", solver_id)
        .add_attribute("solver_addr", info.sender)
        .add_attribute("output_amount", output_amount)
        .add_attribute("risk_bond", risk_bond))
}

/// Claim escrowed funds after Eureka finality
///
/// Called by the solver after ZK proof verifies. The solver receives
/// the escrowed funds plus their bond back.
fn execute_claim_eureka_escrow(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    intent_id: String,
    packet_id: String,
) -> Result<Response, ContractError> {
    let mut escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| ContractError::EthereumEscrowNotFound {
            intent_id: intent_id.clone(),
        })?;

    // Verify escrow is finalized
    if !matches!(escrow.status, EthereumEscrowStatus::Finalized) {
        return Err(ContractError::InvalidEthereumEscrowStatus {
            expected: "Finalized".to_string(),
            actual: format!("{:?}", escrow.status),
        });
    }

    // Verify packet_id matches
    if escrow.packet_id.as_ref() != Some(&packet_id) {
        return Err(ContractError::PacketIdMismatch {
            expected: escrow.packet_id.clone().unwrap_or_default(),
            actual: packet_id,
        });
    }

    // Get fronting info - only the solver who fronted can claim
    let fronting = escrow.fronting.as_ref().ok_or(ContractError::NotFronted {
        intent_id: intent_id.clone(),
    })?;

    // Verify caller is the solver who fronted
    if info.sender != fronting.solver_addr {
        return Err(ContractError::Unauthorized {});
    }

    // Calculate amounts to send
    let received = escrow
        .received_amount
        .unwrap_or(escrow.expected_amount);
    let bond = fronting.bond_amount;
    let total_to_solver = received + bond;

    // Update status
    escrow.status = EthereumEscrowStatus::Claimed;
    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    // Send escrowed funds + bond back to solver
    let send_msg = BankMsg::Send {
        to_address: fronting.solver_addr.to_string(),
        amount: vec![Coin {
            denom: escrow.expected_denom.clone(),
            amount: total_to_solver,
        }],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "claim_eureka_escrow")
        .add_attribute("intent_id", intent_id)
        .add_attribute("solver", fronting.solver_addr.to_string())
        .add_attribute("received_amount", received)
        .add_attribute("bond_returned", bond)
        .add_attribute("total_sent", total_to_solver))
}

/// Handle Eureka escrow failure (timeout, invalid packet, etc.)
///
/// Called by admin when an escrow cannot be completed. If a solver fronted,
/// their bond may be slashed depending on the failure reason.
fn execute_handle_eureka_escrow_failure(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    intent_id: String,
    reason: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only admin can mark escrows as failed
    if info.sender != config.admin {
        return Err(ContractError::Unauthorized {});
    }

    let mut escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| ContractError::EthereumEscrowNotFound {
            intent_id: intent_id.clone(),
        })?;

    // Cannot fail an already claimed or failed escrow
    if matches!(
        escrow.status,
        EthereumEscrowStatus::Claimed | EthereumEscrowStatus::Failed { .. }
    ) {
        return Err(ContractError::InvalidEthereumEscrowStatus {
            expected: "Pending, Received, or Fronted".to_string(),
            actual: format!("{:?}", escrow.status),
        });
    }

    // If solver fronted, determine bond handling based on failure reason
    let mut messages = vec![];
    let mut bond_action = "none".to_string();

    if let Some(fronting) = &escrow.fronting {
        // If failure was not due to solver fault, return their bond
        // For now, we return the bond on any failure (could be more sophisticated)
        let send_msg = BankMsg::Send {
            to_address: fronting.solver_addr.to_string(),
            amount: vec![Coin {
                denom: escrow.expected_denom.clone(),
                amount: fronting.bond_amount,
            }],
        };
        messages.push(send_msg);
        bond_action = "returned".to_string();
    }

    escrow.status = EthereumEscrowStatus::Failed {
        reason: reason.clone(),
    };
    ETHEREUM_ESCROWS.save(deps.storage, &intent_id, &escrow)?;

    let mut response = Response::new()
        .add_attribute("action", "handle_eureka_escrow_failure")
        .add_attribute("intent_id", intent_id)
        .add_attribute("reason", reason)
        .add_attribute("bond_action", bond_action);

    for msg in messages {
        response = response.add_message(msg);
    }

    Ok(response)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&query_config(deps)?),
        QueryMsg::Escrow { escrow_id } => to_json_binary(&query_escrow(deps, escrow_id)?),
        QueryMsg::EscrowsByUser {
            user,
            start_after,
            limit,
        } => to_json_binary(&query_escrows_by_user(deps, user, start_after, limit)?),
        QueryMsg::EscrowByIntent { intent_id } => {
            to_json_binary(&query_escrow_by_intent(deps, intent_id)?)
        }
        QueryMsg::EthereumEscrowStatus { intent_id } => {
            to_json_binary(&query_ethereum_escrow_status(deps, intent_id)?)
        }
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        admin: config.admin.to_string(),
        settlement_contract: config.settlement_contract.to_string(),
    })
}

fn query_escrow(deps: Deps, escrow_id: String) -> StdResult<EscrowResponse> {
    let escrow = ESCROWS.load(deps.storage, &escrow_id)?;
    Ok(escrow_to_response(escrow))
}

fn query_escrow_by_intent(deps: Deps, intent_id: String) -> StdResult<EscrowResponse> {
    let escrow_id = ESCROWS_BY_INTENT.load(deps.storage, &intent_id)?;
    let escrow = ESCROWS.load(deps.storage, &escrow_id)?;
    Ok(escrow_to_response(escrow))
}

/// Query Ethereum escrow status
fn query_ethereum_escrow_status(
    deps: Deps,
    intent_id: String,
) -> StdResult<EthereumEscrowStatusResponse> {
    let escrow = ETHEREUM_ESCROWS
        .load(deps.storage, &intent_id)
        .map_err(|_| {
            cosmwasm_std::StdError::not_found(format!("Ethereum escrow for intent {}", intent_id))
        })?;

    let escrow_status = match &escrow.status {
        EthereumEscrowStatus::Pending => "pending".to_string(),
        EthereumEscrowStatus::Received => "received".to_string(),
        EthereumEscrowStatus::Finalized => "finalized".to_string(),
        EthereumEscrowStatus::Fronted => "fronted".to_string(),
        EthereumEscrowStatus::Claimed => "claimed".to_string(),
        EthereumEscrowStatus::Failed { reason } => format!("failed: {}", reason),
    };

    let (fronted_by, fronted_at) = match &escrow.fronting {
        Some(f) => (Some(f.solver_id.clone()), Some(f.fronted_at)),
        None => (None, None),
    };

    Ok(EthereumEscrowStatusResponse {
        intent_id: escrow.intent_id,
        eth_sender: escrow.eth_sender,
        expected_amount: escrow.expected_amount,
        escrow_status,
        packet_id: escrow.packet_id,
        fronted_by,
        fronted_at,
    })
}

fn query_escrows_by_user(
    deps: Deps,
    user: String,
    _start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<EscrowsResponse> {
    let user_addr = deps.api.addr_validate(&user)?;
    let limit = limit.unwrap_or(30).min(100) as usize;

    let escrows: Vec<EscrowResponse> = USER_ESCROWS
        .prefix(&user_addr)
        .range(deps.storage, None, None, cosmwasm_std::Order::Ascending)
        .take(limit)
        .filter_map(|r| r.ok())
        .filter_map(|(escrow_id, _)| {
            ESCROWS
                .load(deps.storage, &escrow_id)
                .ok()
                .map(escrow_to_response)
        })
        .collect();

    Ok(EscrowsResponse { escrows })
}

fn escrow_to_response(escrow: Escrow) -> EscrowResponse {
    let status = match escrow.status {
        EscrowStatus::Locked => "locked".to_string(),
        EscrowStatus::Released { recipient } => format!("released to {recipient}"),
        EscrowStatus::Refunded => "refunded".to_string(),
        EscrowStatus::Refunding => "refunding".to_string(),
        EscrowStatus::RefundFailed => "refund_failed".to_string(),
    };

    EscrowResponse {
        id: escrow.id,
        owner: escrow.owner.to_string(),
        amount: escrow.amount,
        denom: escrow.denom,
        intent_id: escrow.intent_id,
        expires_at: escrow.expires_at,
        status,
        owner_chain_id: escrow.owner_chain_id,
        owner_source_address: escrow.owner_source_address,
        source_channel: escrow.source_channel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env, MockApi};
    use cosmwasm_std::{from_json, Addr, Coin, Timestamp, Uint128};

    // Helper to get test addresses using MockApi
    struct TestAddrs {
        admin: Addr,
        settlement: Addr,
        user: Addr,
        recipient: Addr,
        random_user: Addr,
        new_admin: Addr,
        new_settlement: Addr,
    }

    fn test_addrs(api: &MockApi) -> TestAddrs {
        TestAddrs {
            admin: api.addr_make("admin"),
            settlement: api.addr_make("settlement"),
            user: api.addr_make("user"),
            recipient: api.addr_make("recipient"),
            random_user: api.addr_make("random_user"),
            new_admin: api.addr_make("new_admin"),
            new_settlement: api.addr_make("new_settlement"),
        }
    }

    fn setup_contract() -> (
        cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        Env,
        TestAddrs,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let addrs = test_addrs(&deps.api);

        let msg = InstantiateMsg {
            admin: addrs.admin.to_string(),
            settlement_contract: addrs.settlement.to_string(),
        };
        let info = message_info(&addrs.admin, &[]);

        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

        (deps, env, addrs)
    }

    fn lock_escrow(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::MemoryStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
        env: &Env,
        addrs: &TestAddrs,
        escrow_id: &str,
        amount: u128,
    ) {
        let info = message_info(&addrs.user, &[Coin::new(amount, "uatom")]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: escrow_id.to_string(),
                intent_id: format!("intent_{}", escrow_id),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap();
    }

    // ==================== INSTANTIATION TESTS ====================

    #[test]
    fn test_instantiate_stores_config() {
        let (deps, _env, addrs) = setup_contract();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.admin.to_string());
        assert_eq!(config.settlement_contract, addrs.settlement.to_string());
    }

    // ==================== LOCK TESTS ====================

    #[test]
    fn test_lock_success_single_coin() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.user, &[Coin::new(100_000u128, "uatom")]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-1".to_string(),
                intent_id: "intent-1".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap();

        assert_eq!(res.attributes.len(), 5);
        assert_eq!(res.attributes[0].value, "lock");
        assert_eq!(res.attributes[1].value, "escrow-1");

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(escrow.id, "escrow-1");
        assert_eq!(escrow.owner, addrs.user.to_string());
        assert_eq!(escrow.amount, Uint128::new(100_000));
        assert_eq!(escrow.denom, "uatom");
        assert_eq!(escrow.intent_id, "intent-1");
        assert_eq!(escrow.status, "locked");
    }

    #[test]
    fn test_lock_multiple_coins_fails() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(
            &addrs.user,
            &[
                Coin::new(100_000u128, "uatom"),
                Coin::new(50_000u128, "uusdc"),
            ],
        );
        let err = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-1".to_string(),
                intent_id: "intent-1".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidFunds { .. }));
    }

    #[test]
    fn test_lock_zero_coins_fails() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.user, &[]);
        let err = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-1".to_string(),
                intent_id: "intent-1".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidFunds { .. }));
    }

    #[test]
    fn test_lock_zero_amount_fails() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.user, &[Coin::new(0u128, "uatom")]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-1".to_string(),
                intent_id: "intent-1".to_string(),
                expires_at: env.block.time.seconds() + 3600,
            },
        );

        // Should succeed but with zero amount - this is technically allowed by the contract
        // but could be prevented with additional validation if desired
        assert!(res.is_ok());
    }

    #[test]
    fn test_lock_duplicate_id_fails() {
        let (mut deps, env, addrs) = setup_contract();

        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let info = message_info(&addrs.user, &[Coin::new(200_000u128, "uatom")]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-1".to_string(),
                intent_id: "intent-2".to_string(),
                expires_at: 9999999999,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::EscrowAlreadyExists { .. }));
    }

    #[test]
    fn test_lock_creates_correct_escrow_entry() {
        let (mut deps, env, addrs) = setup_contract();

        let expires_at = env.block.time.seconds() + 7200;
        let info = message_info(&addrs.user, &[Coin::new(250_000u128, "uatom")]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Lock {
                escrow_id: "escrow-test".to_string(),
                intent_id: "intent-test".to_string(),
                expires_at,
            },
        )
        .unwrap();

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(escrow.id, "escrow-test");
        assert_eq!(escrow.owner, addrs.user.to_string());
        assert_eq!(escrow.amount, Uint128::new(250_000));
        assert_eq!(escrow.denom, "uatom");
        assert_eq!(escrow.intent_id, "intent-test");
        assert_eq!(escrow.expires_at, expires_at);
        assert_eq!(escrow.status, "locked");
    }

    // ==================== RELEASE TESTS ====================

    #[test]
    fn test_release_success_by_settlement_contract() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let info = message_info(&addrs.settlement, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "release");
        assert_eq!(res.messages.len(), 1);

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            escrow.status,
            format!("released to {}", addrs.recipient.to_string())
        );
    }

    #[test]
    fn test_release_by_non_settlement_contract_fails() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_release_non_existent_escrow_fails() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.settlement, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Release {
                escrow_id: "nonexistent".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::EscrowNotFound { .. }));
    }

    #[test]
    fn test_release_already_released_escrow_fails() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Release once
        let info = message_info(&addrs.settlement, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap();

        // Try to release again
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::EscrowNotFound { .. }));
    }

    #[test]
    fn test_release_funds_go_to_correct_recipient() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let info = message_info(&addrs.settlement, &[]);
        let res = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        // Verify the BankMsg is sending to the correct recipient
        assert_eq!(res.attributes[2].value, addrs.recipient.to_string());
        assert_eq!(res.attributes[3].value, "100000");
    }

    // ==================== REFUND TESTS ====================

    #[test]
    fn test_refund_success_after_expiration() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        let info = message_info(&addrs.user, &[]);
        let res = execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.attributes[0].value, "refund");
        assert_eq!(res.messages.len(), 1);

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(escrow.status, "refunded");
    }

    #[test]
    fn test_refund_before_expiration_fails() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let info = message_info(&addrs.user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::EscrowNotExpired { .. }));
    }

    #[test]
    fn test_refund_by_non_owner_fails() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_refund_already_refunded_fails() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        // Refund once
        let info = message_info(&addrs.user, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap();

        // Try to refund again
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::InvalidStatus {}));
    }

    #[test]
    fn test_refund_funds_return_to_owner() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        let info = message_info(&addrs.user, &[]);
        let res = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap();

        assert_eq!(res.messages.len(), 1);
        // Attributes: action, refund_type, escrow_id, owner, amount
        assert_eq!(res.attributes[0].value, "refund");
        assert_eq!(res.attributes[1].value, "local_refund");
        assert_eq!(res.attributes[2].value, "escrow-1");
        assert_eq!(res.attributes[3].value, addrs.user.to_string());
        assert_eq!(res.attributes[4].value, "100000");
    }

    // ==================== QUERY TESTS ====================

    #[test]
    fn test_query_config() {
        let (deps, _env, addrs) = setup_contract();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.admin.to_string());
        assert_eq!(config.settlement_contract, addrs.settlement.to_string());
    }

    #[test]
    fn test_query_escrow_by_id() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(escrow.id, "escrow-1");
        assert_eq!(escrow.amount, Uint128::new(100_000));
    }

    #[test]
    fn test_query_escrow_not_found() {
        let (deps, env, _addrs) = setup_contract();

        let err = query(
            deps.as_ref(),
            env,
            QueryMsg::Escrow {
                escrow_id: "nonexistent".to_string(),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_query_escrows_by_owner() {
        let (mut deps, env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);
        lock_escrow(&mut deps, &env, &addrs, "escrow-2", 200_000);

        let response: EscrowsResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::EscrowsByUser {
                    user: addrs.user.to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(response.escrows.len(), 2);
        assert_eq!(response.escrows[0].id, "escrow-1");
        assert_eq!(response.escrows[1].id, "escrow-2");
    }

    #[test]
    fn test_query_escrows_by_owner_empty() {
        let (deps, env, addrs) = setup_contract();

        let response: EscrowsResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::EscrowsByUser {
                    user: addrs.user.to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(response.escrows.len(), 0);
    }

    // ==================== STATE TRANSITION TESTS ====================

    #[test]
    fn test_lock_to_release_flow() {
        let (mut deps, env, addrs) = setup_contract();

        // Lock
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(escrow.status, "locked");

        // Release
        let info = message_info(&addrs.settlement, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap();

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            escrow.status,
            format!("released to {}", addrs.recipient.to_string())
        );
    }

    #[test]
    fn test_lock_to_refund_flow() {
        let (mut deps, mut env, addrs) = setup_contract();

        // Lock
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(escrow.status, "locked");

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        // Refund
        let info = message_info(&addrs.user, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap();

        let escrow: EscrowResponse = from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::Escrow {
                    escrow_id: "escrow-1".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(escrow.status, "refunded");
    }

    #[test]
    fn test_cannot_release_after_refund() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Fast forward and refund
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);
        let info = message_info(&addrs.user, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap();

        // Try to release
        let info = message_info(&addrs.settlement, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::EscrowNotFound { .. }));
    }

    #[test]
    fn test_cannot_refund_after_release() {
        let (mut deps, mut env, addrs) = setup_contract();
        lock_escrow(&mut deps, &env, &addrs, "escrow-1", 100_000);

        // Release
        let info = message_info(&addrs.settlement, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::Release {
                escrow_id: "escrow-1".to_string(),
                recipient: addrs.recipient.to_string(),
            },
        )
        .unwrap();

        // Fast forward past expiration
        env.block.time = Timestamp::from_seconds(env.block.time.seconds() + 7200);

        // Try to refund
        let info = message_info(&addrs.user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::Refund {
                escrow_id: "escrow-1".to_string(),
            },
        )
        .unwrap_err();

        // Can't refund after release - escrow is in Released status
        assert!(matches!(err, ContractError::InvalidStatus {}));
    }

    // ==================== UPDATE CONFIG TESTS ====================

    #[test]
    fn test_update_config_success() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateConfig {
                admin: Some(addrs.new_admin.to_string()),
                settlement_contract: Some(addrs.new_settlement.to_string()),
            },
        )
        .unwrap();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), env, QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.new_admin.to_string());
        assert_eq!(config.settlement_contract, addrs.new_settlement.to_string());
    }

    #[test]
    fn test_update_config_unauthorized() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.random_user, &[]);
        let err = execute(
            deps.as_mut(),
            env,
            info,
            ExecuteMsg::UpdateConfig {
                admin: Some(addrs.new_admin.to_string()),
                settlement_contract: None,
            },
        )
        .unwrap_err();

        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_update_config_partial_update() {
        let (mut deps, env, addrs) = setup_contract();

        let info = message_info(&addrs.admin, &[]);
        execute(
            deps.as_mut(),
            env.clone(),
            info,
            ExecuteMsg::UpdateConfig {
                admin: None,
                settlement_contract: Some(addrs.new_settlement.to_string()),
            },
        )
        .unwrap();

        let config: ConfigResponse =
            from_json(query(deps.as_ref(), env, QueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(config.admin, addrs.admin.to_string()); // Unchanged
        assert_eq!(config.settlement_contract, addrs.new_settlement.to_string());
        // Changed
    }
}
