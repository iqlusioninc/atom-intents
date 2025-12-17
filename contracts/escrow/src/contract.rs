use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult,
};

use crate::error::ContractError;
use crate::msg::{ConfigResponse, EscrowResponse, EscrowsResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{Config, Escrow, EscrowStatus, CONFIG, ESCROWS, USER_ESCROWS};

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
        ExecuteMsg::Release {
            escrow_id,
            recipient,
        } => execute_release(deps, env, info, escrow_id, recipient),
        ExecuteMsg::Refund { escrow_id } => execute_refund(deps, env, info, escrow_id),
        ExecuteMsg::UpdateConfig {
            admin,
            settlement_contract,
        } => execute_update_config(deps, info, admin, settlement_contract),
    }
}

fn execute_lock(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    escrow_id: String,
    intent_id: String,
    expires_at: u64,
) -> Result<Response, ContractError> {
    // Verify escrow doesn't exist
    if ESCROWS.has(deps.storage, &escrow_id) {
        return Err(ContractError::EscrowAlreadyExists { id: escrow_id });
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
        intent_id,
        expires_at,
        status: EscrowStatus::Locked,
    };

    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;
    USER_ESCROWS.save(deps.storage, (&info.sender, &escrow_id), &true)?;

    Ok(Response::new()
        .add_attribute("action", "lock")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("owner", info.sender)
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

    let mut escrow = ESCROWS
        .load(deps.storage, &escrow_id)
        .map_err(|_| ContractError::EscrowNotFound { id: escrow_id.clone() })?;

    // Check not already released
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::EscrowNotFound { id: escrow_id });
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
    let mut escrow = ESCROWS
        .load(deps.storage, &escrow_id)
        .map_err(|_| ContractError::EscrowNotFound { id: escrow_id.clone() })?;

    // Only owner can refund
    if info.sender != escrow.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Check escrow is expired
    if env.block.time.seconds() < escrow.expires_at {
        return Err(ContractError::EscrowNotExpired { id: escrow_id });
    }

    // Check not already released
    if !matches!(escrow.status, EscrowStatus::Locked) {
        return Err(ContractError::EscrowNotFound { id: escrow_id });
    }

    // Update status
    escrow.status = EscrowStatus::Refunded;
    ESCROWS.save(deps.storage, &escrow_id, &escrow)?;

    // Send funds back to owner
    let send_msg = BankMsg::Send {
        to_address: escrow.owner.to_string(),
        amount: vec![Coin {
            denom: escrow.denom.clone(),
            amount: escrow.amount,
        }],
    };

    Ok(Response::new()
        .add_message(send_msg)
        .add_attribute("action", "refund")
        .add_attribute("escrow_id", escrow_id)
        .add_attribute("owner", escrow.owner)
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
        EscrowStatus::Released { recipient } => format!("released to {}", recipient),
        EscrowStatus::Refunded => "refunded".to_string(),
    };

    EscrowResponse {
        id: escrow.id,
        owner: escrow.owner.to_string(),
        amount: escrow.amount,
        denom: escrow.denom,
        intent_id: escrow.intent_id,
        expires_at: escrow.expires_at,
        status,
    }
}
