/// Tier 2 Regression Tests: Settlement State Machine (ST-H1)
///
/// Tests that SettlementStatus::can_transition_to() correctly validates
/// state transitions and prevents invalid ones.

use atom_intents_types::SettlementStatus;

// ═══════════════════════════════════════════════════════════════════════════
// VALID TRANSITIONS (Happy Path)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_valid_happy_path_transitions() {
    // Pending -> UserLocked
    assert!(SettlementStatus::Pending.can_transition_to(&SettlementStatus::UserLocked));
    // UserLocked -> SolverLocked
    assert!(SettlementStatus::UserLocked.can_transition_to(&SettlementStatus::SolverLocked));
    // SolverLocked -> Executing
    assert!(SettlementStatus::SolverLocked.can_transition_to(&SettlementStatus::Executing));
    // Executing -> Complete
    assert!(SettlementStatus::Executing.can_transition_to(&SettlementStatus::Complete));
}

// ═══════════════════════════════════════════════════════════════════════════
// VALID TRANSITIONS (Failure/Timeout from active states)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_failure_reachable_from_all_active_states() {
    let fail = SettlementStatus::Failed {
        reason: "test".into(),
    };

    assert!(SettlementStatus::Pending.can_transition_to(&fail));
    assert!(SettlementStatus::UserLocked.can_transition_to(&fail));
    assert!(SettlementStatus::SolverLocked.can_transition_to(&fail));
    assert!(SettlementStatus::Executing.can_transition_to(&fail));
}

#[test]
fn test_timeout_reachable_from_all_active_states() {
    assert!(SettlementStatus::Pending.can_transition_to(&SettlementStatus::TimedOut));
    assert!(SettlementStatus::UserLocked.can_transition_to(&SettlementStatus::TimedOut));
    assert!(SettlementStatus::SolverLocked.can_transition_to(&SettlementStatus::TimedOut));
    assert!(SettlementStatus::Executing.can_transition_to(&SettlementStatus::TimedOut));
}

// ═══════════════════════════════════════════════════════════════════════════
// INVALID TRANSITIONS (Must be rejected)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_skip_transitions_rejected() {
    // Can't skip UserLocked
    assert!(!SettlementStatus::Pending.can_transition_to(&SettlementStatus::SolverLocked));
    // Can't skip SolverLocked
    assert!(!SettlementStatus::UserLocked.can_transition_to(&SettlementStatus::Executing));
    // Can't skip Executing
    assert!(!SettlementStatus::SolverLocked.can_transition_to(&SettlementStatus::Complete));
    // Can't go directly from Pending to Complete
    assert!(!SettlementStatus::Pending.can_transition_to(&SettlementStatus::Complete));
}

#[test]
fn test_backward_transitions_rejected() {
    // Complete is terminal
    assert!(!SettlementStatus::Complete.can_transition_to(&SettlementStatus::Executing));
    assert!(!SettlementStatus::Complete.can_transition_to(&SettlementStatus::Pending));

    // Failed is terminal
    let fail = SettlementStatus::Failed {
        reason: "test".into(),
    };
    assert!(!fail.can_transition_to(&SettlementStatus::Pending));
    assert!(!fail.can_transition_to(&SettlementStatus::UserLocked));
    assert!(!fail.can_transition_to(&SettlementStatus::Executing));

    // TimedOut is terminal
    assert!(!SettlementStatus::TimedOut.can_transition_to(&SettlementStatus::Pending));
    assert!(!SettlementStatus::TimedOut.can_transition_to(&SettlementStatus::Executing));
}

#[test]
fn test_self_transitions_rejected() {
    assert!(!SettlementStatus::Pending.can_transition_to(&SettlementStatus::Pending));
    assert!(!SettlementStatus::UserLocked.can_transition_to(&SettlementStatus::UserLocked));
    assert!(!SettlementStatus::Executing.can_transition_to(&SettlementStatus::Executing));
    assert!(!SettlementStatus::Complete.can_transition_to(&SettlementStatus::Complete));
}

#[test]
fn test_terminal_to_terminal_rejected() {
    let fail = SettlementStatus::Failed {
        reason: "test".into(),
    };
    assert!(!SettlementStatus::Complete.can_transition_to(&fail));
    assert!(!SettlementStatus::Complete.can_transition_to(&SettlementStatus::TimedOut));
    assert!(!fail.can_transition_to(&SettlementStatus::TimedOut));
    assert!(!SettlementStatus::TimedOut.can_transition_to(&fail));
}
