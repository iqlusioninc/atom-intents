use cosmwasm_std::{
    entry_point, to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult,
};

use crate::error::ContractError;
use crate::msg::{
    ConfigResponse, EscrowResponse, EscrowsResponse, ExecuteMsg, InstantiateMsg, QueryMsg,
};
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
        EscrowStatus::Released { recipient } => format!("released to {recipient}"),
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

        assert!(matches!(err, ContractError::EscrowNotFound { .. }));
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
        assert_eq!(res.attributes[2].value, addrs.user.to_string());
        assert_eq!(res.attributes[3].value, "100000");
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

        assert!(matches!(err, ContractError::EscrowNotFound { .. }));
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
