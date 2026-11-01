use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const NEAR: u128 = 1_000_000_000_000_000_000_000_000;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn ctx(predecessor: &str, deposit: u128, ts_secs: u64) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc("njangi.testnet"))
        .predecessor_account_id(acc(predecessor))
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(ts_secs * 1_000_000_000)
        .prepaid_gas(Gas::from_tgas(300));
    testing_env!(b.build());
}

fn members() -> Vec<AccountId> {
    vec![acc("ama.testnet"), acc("bih.testnet"), acc("chi.testnet")]
}

fn keeper_policy() -> serde_json::Value {
    json!({"roles": [{"name": "keeper", "kind": {"Group": ["njangi.testnet"]}}]})
}

/// Creates a 3-member circle (1 NEAR, 100 s rounds, 5 % fund) and activates it.
fn setup() -> Contract {
    ctx("owner.testnet", 0, 1000);
    let mut c = Contract::new(acc("sputnikv2.testnet"));
    ctx("ama.testnet", 6 * NEAR, 1000);
    let id = c.create_circle("Mbanya family".into(), "mbanya-fam".into(), members(), U128(NEAR), U64(100), 500, 0);
    assert_eq!(id, 0);
    ctx("njangi.testnet", 0, 1001);
    assert!(c.on_dao_checked(0, Ok(keeper_policy())));
    ctx("bih.testnet", 0, 1002);
    c.join(0);
    assert_eq!(c.get_circle(0).unwrap().status, CircleStatus::Forming);
    ctx("chi.testnet", 0, 1003);
    c.join(0);
    c
}

#[test]
fn creates_dao_and_activates_when_everyone_joined() {
    let c = setup();
    let circle = c.get_circle(0).unwrap();
    assert_eq!(circle.status, CircleStatus::Active);
    assert_eq!(circle.dao.as_str(), "mbanya-fam.sputnikv2.testnet");
    assert_eq!(circle.start_ns.0, 1003 * 1_000_000_000);
    assert_eq!(c.get_member_circles(acc("chi.testnet")), vec![0]);
    // pot = 3 NEAR - 5 %
    assert_eq!(circle.pot(), 3 * NEAR * 95 / 100);
}

#[test]
fn wrong_keeper_policy_keeps_circle_forming() {
    ctx("owner.testnet", 0, 1000);
    let mut c = Contract::new(acc("sputnikv2.testnet"));
    ctx("ama.testnet", 6 * NEAR, 1000);
    c.create_circle("x".into(), "x".into(), members(), U128(NEAR), U64(100), 0, 0);
    ctx("njangi.testnet", 0, 1001);
    let bad = json!({"roles": [{"name": "keeper", "kind": {"Group": ["njangi.testnet", "evil.testnet"]}}]});
    assert!(!c.on_dao_checked(0, Ok(bad)));
    assert!(!c.get_circle(0).unwrap().dao_ready);
}

#[test]
#[should_panic(expected = "Attach the DAO deposit")]
fn create_requires_dao_deposit() {
    ctx("owner.testnet", 0, 1000);
    let mut c = Contract::new(acc("sputnikv2.testnet"));
    ctx("ama.testnet", NEAR, 1000);
    c.create_circle("x".into(), "x".into(), members(), U128(NEAR), U64(100), 0, 0);
}

#[test]
#[should_panic(expected = "Creator must be a member")]
fn creator_must_be_member() {
    ctx("owner.testnet", 0, 1000);
    let mut c = Contract::new(acc("sputnikv2.testnet"));
    ctx("zed.testnet", 6 * NEAR, 1000);
    c.create_circle("x".into(), "x".into(), members(), U128(NEAR), U64(100), 0, 0);
}

#[test]
#[should_panic(expected = "Attach exactly the contribution amount")]
fn contribution_must_be_exact() {
    let mut c = setup();
    ctx("ama.testnet", NEAR / 2, 1010);
    c.contribute(0);
}

#[test]
#[should_panic(expected = "Already settled")]
fn cannot_pay_twice() {
    let mut c = setup();
    ctx("ama.testnet", NEAR, 1010);
    c.contribute(0);
    ctx("ama.testnet", NEAR, 1011);
    c.contribute(0);
}

#[test]
fn full_round_on_time_then_late_and_reputation() {
    let mut c = setup();
    // Round 0, everyone on time (deadline = 1003 + 100).
    for (who, t) in [("ama.testnet", 1010), ("bih.testnet", 1020), ("chi.testnet", 1030)] {
        ctx(who, NEAR, t);
        c.contribute(0);
    }
    let circle = c.get_circle(0).unwrap();
    assert!(circle.payout_pending, "last payment starts the payout");
    // DAO accepted the proposal (id 7) and the keeper approved it.
    ctx("njangi.testnet", 0, 1031);
    assert!(c.on_payout_done(0, 0, acc("ama.testnet"), U128(circle.pot()), U64(7), Ok(())));
    let circle = c.get_circle(0).unwrap();
    assert_eq!(circle.round, 1);
    assert!(!circle.payout_pending);
    assert_eq!(circle.payouts.len(), 1);
    assert_eq!(circle.payouts[0].beneficiary, acc("ama.testnet"));

    // Round 1 deadline = 1003 + 200. chi pays late.
    ctx("ama.testnet", NEAR, 1100);
    c.contribute(0);
    ctx("bih.testnet", NEAR, 1150);
    c.contribute(0);
    ctx("njangi.testnet", 0, 1250);
    let rv = c.get_round(0);
    assert!(rv.overdue);
    assert_eq!(rv.unpaid, vec![acc("chi.testnet")]);
    assert_eq!(rv.beneficiary, Some(acc("bih.testnet")));
    ctx("chi.testnet", NEAR, 1250);
    c.contribute(0);
    let r = c.get_reputation(acc("chi.testnet"));
    assert_eq!(r.on_time, 1);
    assert_eq!(r.late, 1);
    assert_eq!(r.score, SCORE_ON_TIME + SCORE_LATE);
    assert_eq!(r.history.last().unwrap().kind, RepKind::Late);
    assert_eq!(r.history.last().unwrap().round, 1);
    assert_eq!(c.get_reputation(acc("ama.testnet")).score, 2 * SCORE_ON_TIME);
}

#[test]
fn failed_approval_allows_release_retry() {
    let mut c = setup();
    for who in ["ama.testnet", "bih.testnet", "chi.testnet"] {
        ctx(who, NEAR, 1010);
        c.contribute(0);
    }
    ctx("njangi.testnet", 0, 1011);
    assert!(!c.on_payout_done(0, 0, acc("ama.testnet"), U128(1), U64(1), Err(PromiseError::Failed)));
    let circle = c.get_circle(0).unwrap();
    assert_eq!(circle.round, 0);
    assert!(!circle.payout_pending);
    ctx("zed.testnet", 0, 1012);
    c.release(0);
    assert!(c.get_circle(0).unwrap().payout_pending);
}

#[test]
fn completes_after_every_member_received() {
    let mut c = setup();
    for round in 0..3u32 {
        for who in ["ama.testnet", "bih.testnet", "chi.testnet"] {
            ctx(who, NEAR, 1010 + round as u64 * 100);
            c.contribute(0);
        }
        let ben = c.get_circle(0).unwrap().members[round as usize].clone();
        ctx("njangi.testnet", 0, 1050 + round as u64 * 100);
        assert!(c.on_payout_done(0, round, ben, U128(1), U64(round as u64), Ok(())));
    }
    let circle = c.get_circle(0).unwrap();
    assert_eq!(circle.status, CircleStatus::Completed);
    let r = c.get_reputation(acc("bih.testnet"));
    assert_eq!(r.completed, 1);
    assert_eq!(r.received, 1);
    assert_eq!(r.score, 3 * SCORE_ON_TIME + SCORE_COMPLETED);
}

#[test]
fn dao_can_swap_future_turns() {
    let mut c = setup();
    ctx("mbanya-fam.sputnikv2.testnet", 0, 1010);
    c.dao_swap_turns(0, 0, 2);
    let circle = c.get_circle(0).unwrap();
    assert_eq!(circle.members[0], acc("chi.testnet"));
    assert_eq!(circle.members[2], acc("ama.testnet"));
    assert_eq!(c.get_round(0).beneficiary, Some(acc("chi.testnet")));
}

#[test]
#[should_panic(expected = "Only the circle's DAO")]
fn only_dao_can_swap() {
    let mut c = setup();
    ctx("ama.testnet", 0, 1010);
    c.dao_swap_turns(0, 0, 2);
}

#[test]
fn dao_covers_overdue_default_then_release() {
    let mut c = setup();
    ctx("ama.testnet", NEAR, 1010);
    c.contribute(0);
    ctx("bih.testnet", NEAR, 1020);
    c.contribute(0);
    ctx("mbanya-fam.sputnikv2.testnet", 0, 1200);
    c.dao_cover_default(0, acc("chi.testnet"));
    let r = c.get_reputation(acc("chi.testnet"));
    assert_eq!(r.defaults, 1);
    assert_eq!(r.score, SCORE_DEFAULT);
    assert_eq!(c.get_round(0).covered, vec![acc("chi.testnet")]);
    ctx("anyone.testnet", 0, 1201);
    c.release(0);
    assert!(c.get_circle(0).unwrap().payout_pending);
}

#[test]
#[should_panic(expected = "Round is not overdue yet")]
fn cannot_cover_before_deadline() {
    let mut c = setup();
    ctx("mbanya-fam.sputnikv2.testnet", 0, 1050);
    c.dao_cover_default(0, acc("chi.testnet"));
}

#[test]
#[should_panic(expected = "reputation is below")]
fn reputation_gate_blocks_join() {
    let mut c = setup();
    // chi defaults in circle 0
    ctx("ama.testnet", NEAR, 1010);
    c.contribute(0);
    ctx("bih.testnet", NEAR, 1020);
    c.contribute(0);
    ctx("mbanya-fam.sputnikv2.testnet", 0, 1200);
    c.dao_cover_default(0, acc("chi.testnet"));
    // ama starts a circle that needs score >= 10
    ctx("ama.testnet", 6 * NEAR, 1300);
    c.create_circle("Trusted".into(), "trusted".into(), members(), U128(NEAR), U64(100), 0, 10);
    ctx("chi.testnet", 0, 1301);
    c.join(1);
}

#[test]
fn open_circle_admits_members_with_a_bad_record() {
    let mut c = setup();
    ctx("ama.testnet", NEAR, 1010);
    c.contribute(0);
    ctx("bih.testnet", NEAR, 1020);
    c.contribute(0);
    ctx("mbanya-fam.sputnikv2.testnet", 0, 1200);
    c.dao_cover_default(0, acc("chi.testnet"));
    assert!(c.get_reputation(acc("chi.testnet")).score < 0);
    // min_reputation 0 means open to everyone
    ctx("ama.testnet", 6 * NEAR, 1300);
    c.create_circle("Open".into(), "open".into(), members(), U128(NEAR), U64(100), 0, 0);
    ctx("chi.testnet", 0, 1301);
    c.join(1);
    assert!(c.get_circle(1).unwrap().joined.iter().filter(|j| **j).count() >= 2);
}
