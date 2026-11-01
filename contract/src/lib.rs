//! Njangi: rotating savings circles (njangi / tontine / ROSCA) whose money lives in a Sputnik DAO.
//!
//! * Every circle gets its own Sputnik DAO, created in the same transaction as the circle.
//! * Contributions are forwarded straight into the DAO treasury. This contract never keeps them.
//! * This contract is the only member of the DAO's `keeper` role, which may only add and approve
//!   `transfer` proposals. The code below only ever proposes "full pot -> this round's beneficiary",
//!   and only once every member has paid (or the members voted to cover a defaulter).
//! * Members hold the `members` role: majority vote on everything else (swap turns, cover a
//!   default, emergency loans from the social fund, policy changes).
//! * Reputation lives on named accounts and every point points at a (circle, round).

use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::serde_json::{self, json};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    env, log, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError,
};

pub const MAX_MEMBERS: usize = 20;
pub const MIN_MEMBERS: usize = 2;
pub const MAX_HISTORY: usize = 40;
pub const MIN_PERIOD_SECS: u64 = 60;
pub const MAX_FUND_BPS: u16 = 2_000;

const GAS_CREATE_DAO: Gas = Gas::from_tgas(150);
const GAS_ON_DAO_CREATED: Gas = Gas::from_tgas(30);
const GAS_DAO_VIEW: Gas = Gas::from_tgas(10);
const GAS_ON_DAO_CHECKED: Gas = Gas::from_tgas(10);
const GAS_ADD_PROPOSAL: Gas = Gas::from_tgas(20);
const GAS_ON_PROPOSED: Gas = Gas::from_tgas(90);
const GAS_ACT_PROPOSAL: Gas = Gas::from_tgas(50);
const GAS_ON_PAID: Gas = Gas::from_tgas(15);

pub const SCORE_ON_TIME: i64 = 10;
pub const SCORE_LATE: i64 = -15;
pub const SCORE_DEFAULT: i64 = -50;
pub const SCORE_COMPLETED: i64 = 20;

#[derive(BorshStorageKey)]
#[near]
enum Key {
    Circles,
    Reputation,
    MemberCircles,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum CircleStatus {
    /// Waiting for the DAO to exist and for every invited member to join.
    Forming,
    /// Rounds are running.
    Active,
    /// Every member has received the pot once.
    Completed,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Payout {
    pub round: u32,
    pub beneficiary: AccountId,
    pub amount: U128,
    pub proposal_id: U64,
    pub timestamp_ns: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Circle {
    pub id: u32,
    pub name: String,
    pub creator: AccountId,
    pub dao: AccountId,
    /// Rotation order: `members[r]` receives the pot in round `r`.
    pub members: Vec<AccountId>,
    pub joined: Vec<bool>,
    /// yoctoNEAR each member pays per round.
    pub contribution: U128,
    pub period_secs: U64,
    /// Share of every contribution kept in the DAO as the circle's social fund (basis points).
    pub fund_bps: u16,
    pub min_reputation: i64,
    pub status: CircleStatus,
    pub dao_ready: bool,
    pub created_ns: U64,
    /// When round 0 opened. 0 while forming.
    pub start_ns: U64,
    pub round: u32,
    /// Per member, for the current round.
    pub paid: Vec<bool>,
    /// Per member, for the current round: the members voted to cover this share from the social fund.
    pub covered: Vec<bool>,
    pub payout_pending: bool,
    pub payouts: Vec<Payout>,
    /// Total yoctoNEAR forwarded to the DAO by this circle.
    pub collected: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq)]
pub enum RepKind {
    OnTime,
    Late,
    Default,
    Received,
    Completed,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct RepEvent {
    pub kind: RepKind,
    pub circle_id: u32,
    pub round: u32,
    pub points: i64,
    pub timestamp_ns: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct Reputation {
    pub score: i64,
    pub on_time: u32,
    pub late: u32,
    pub defaults: u32,
    pub received: u32,
    pub completed: u32,
    /// Most recent events, newest last (capped at MAX_HISTORY).
    pub history: Vec<RepEvent>,
}

#[near(serializers = [json])]
pub struct RoundView {
    pub circle_id: u32,
    pub round: u32,
    pub beneficiary: Option<AccountId>,
    pub deadline_ns: U64,
    pub paid: Vec<AccountId>,
    pub covered: Vec<AccountId>,
    pub unpaid: Vec<AccountId>,
    pub pot: U128,
    pub overdue: bool,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    pub owner: AccountId,
    pub factory: AccountId,
    pub dao_deposit: NearToken,
    circles: Vector<Circle>,
    reputation: LookupMap<AccountId, Reputation>,
    member_circles: LookupMap<AccountId, Vec<u32>>,
}

fn emit(event: &str, data: serde_json::Value) {
    log!(
        "EVENT_JSON:{}",
        json!({"standard": "njangi", "version": "1.0.0", "event": event, "data": [data]})
    );
}

impl Circle {
    pub fn n(&self) -> u128 {
        self.members.len() as u128
    }
    pub fn index_of(&self, a: &AccountId) -> Option<usize> {
        self.members.iter().position(|m| m == a)
    }
    /// What the beneficiary receives each round.
    pub fn pot(&self) -> u128 {
        let gross = self.contribution.0 * self.n();
        gross - gross * self.fund_bps as u128 / 10_000
    }
    pub fn deadline_ns(&self) -> u64 {
        self.start_ns.0 + (self.round as u64 + 1) * self.period_secs.0 * 1_000_000_000
    }
    pub fn round_settled(&self) -> bool {
        self.paid.iter().zip(self.covered.iter()).all(|(p, c)| *p || *c)
    }
}

#[near]
impl Contract {
    #[init]
    pub fn new(factory: AccountId) -> Self {
        Self {
            owner: env::predecessor_account_id(),
            factory,
            dao_deposit: NearToken::from_near(6),
            circles: Vector::new(Key::Circles),
            reputation: LookupMap::new(Key::Reputation),
            member_circles: LookupMap::new(Key::MemberCircles),
        }
    }

    pub fn set_dao_deposit(&mut self, yocto: U128) {
        require!(env::predecessor_account_id() == self.owner, "Only owner");
        self.dao_deposit = NearToken::from_yoctonear(yocto.0);
    }

    // ------------------------------------------------------------------ circles

    /// Creates a circle and its Sputnik DAO (`<dao_name>.<factory>`) in one transaction.
    /// Attach at least `dao_deposit` (pays the DAO's storage). The caller must be in `members`
    /// and is joined automatically; everyone else accepts with `join`.
    #[payable]
    pub fn create_circle(
        &mut self,
        name: String,
        dao_name: String,
        members: Vec<AccountId>,
        contribution: U128,
        period_secs: U64,
        fund_bps: u16,
        min_reputation: i64,
    ) -> u32 {
        let creator = env::predecessor_account_id();
        let deposit = env::attached_deposit();
        require!(deposit >= self.dao_deposit, "Attach the DAO deposit (see dao_deposit)");
        require!(!name.trim().is_empty() && name.len() <= 64, "Name must be 1-64 chars");
        require!(
            !dao_name.is_empty()
                && dao_name.len() <= 32
                && dao_name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
            "dao_name: 1-32 chars of a-z, 0-9, - or _"
        );
        require!(
            members.len() >= MIN_MEMBERS && members.len() <= MAX_MEMBERS,
            "A circle needs 2-20 members"
        );
        for (i, m) in members.iter().enumerate() {
            require!(!members[..i].contains(m), "Duplicate member");
        }
        require!(members.contains(&creator), "Creator must be a member");
        require!(
            contribution.0 >= NearToken::from_millinear(10).as_yoctonear(),
            "Contribution must be at least 0.01 NEAR"
        );
        require!(period_secs.0 >= MIN_PERIOD_SECS, "Period must be at least 60 seconds");
        require!(fund_bps <= MAX_FUND_BPS, "Social fund share is at most 20%");
        require!(
            self.score_of(&creator) >= min_reputation,
            "Your reputation is below this circle's minimum"
        );

        let id = self.circles.len();
        let dao: AccountId = format!("{}.{}", dao_name, self.factory)
            .parse()
            .unwrap_or_else(|_| env::panic_str("Invalid DAO account"));
        let n = members.len();
        let mut joined = vec![false; n];
        joined[members.iter().position(|m| m == &creator).unwrap()] = true;

        let circle = Circle {
            id,
            name: name.clone(),
            creator: creator.clone(),
            dao: dao.clone(),
            members: members.clone(),
            joined,
            contribution,
            period_secs,
            fund_bps,
            min_reputation,
            status: CircleStatus::Forming,
            dao_ready: false,
            created_ns: U64(env::block_timestamp()),
            start_ns: U64(0),
            round: 0,
            paid: vec![false; n],
            covered: vec![false; n],
            payout_pending: false,
            payouts: vec![],
            collected: U128(0),
        };
        self.circles.push(circle);
        for m in &members {
            let mut list = self.member_circles.get(m).cloned().unwrap_or_default();
            list.push(id);
            self.member_circles.insert(m.clone(), list);
        }

        // The DAO policy: this contract is the keeper (transfers only), members decide the rest.
        let policy = json!({
            "roles": [
                {
                    "name": "keeper",
                    "kind": {"Group": [env::current_account_id()]},
                    "permissions": ["transfer:AddProposal", "transfer:VoteApprove"],
                    "vote_policy": {"transfer": {"weight_kind": "RoleWeight", "quorum": "0", "threshold": [1, 1]}}
                },
                {
                    "name": "members",
                    "kind": {"Group": members},
                    "permissions": ["*:AddProposal", "*:VoteApprove", "*:VoteReject", "*:VoteRemove", "*:Finalize"],
                    "vote_policy": {}
                }
            ],
            "default_vote_policy": {"weight_kind": "RoleWeight", "quorum": "0", "threshold": [1, 2]},
            "proposal_bond": "0",
            "proposal_period": "604800000000000",
            "bounty_bond": "0",
            "bounty_forgiveness_period": "86400000000000"
        });
        let dao_args = json!({
            "config": {"name": dao_name, "purpose": format!("Njangi circle: {}", name), "metadata": ""},
            "policy": policy
        });
        let args = json!({
            "name": dao_name,
            "args": Base64VecU8(serde_json::to_vec(&dao_args).unwrap())
        });
        emit(
            "circle_created",
            json!({"circle_id": id, "name": name, "creator": creator, "dao": dao, "members": members}),
        );
        Promise::new(self.factory.clone())
            .function_call(
                "create",
                serde_json::to_vec(&args).unwrap(),
                deposit,
                GAS_CREATE_DAO,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_DAO_CREATED)
                    .on_dao_created(id),
            )
            .detach();
        id
    }

    #[private]
    pub fn on_dao_created(&mut self, circle_id: u32) {
        // The factory does not wait for the account to exist, so check the DAO's policy next.
        let dao = self.circle(circle_id).dao.clone();
        Promise::new(dao)
            .function_call("get_policy", b"{}".to_vec(), NearToken::from_yoctonear(0), GAS_DAO_VIEW)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_DAO_CHECKED)
                    .on_dao_checked(circle_id),
            )
            .detach();
    }

    /// Anyone can re-check a forming circle's DAO (e.g. if the first check ran too early).
    pub fn confirm_dao(&mut self, circle_id: u32) -> Promise {
        let c = self.circle(circle_id);
        require!(!c.dao_ready, "DAO already confirmed");
        Promise::new(c.dao.clone())
            .function_call("get_policy", b"{}".to_vec(), NearToken::from_yoctonear(0), GAS_DAO_VIEW)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_DAO_CHECKED)
                    .on_dao_checked(circle_id),
            )
    }

    #[private]
    pub fn on_dao_checked(
        &mut self,
        circle_id: u32,
        #[callback_result] policy: Result<serde_json::Value, PromiseError>,
    ) -> bool {
        let me = env::current_account_id();
        let ok = match policy {
            Ok(p) => p["roles"].as_array().map_or(false, |roles| {
                roles.iter().any(|r| {
                    r["name"] == "keeper"
                        && r["kind"]["Group"]
                            .as_array()
                            .map_or(false, |g| g.len() == 1 && g[0] == me.as_str())
                })
            }),
            Err(_) => false,
        };
        let c = self.circles.get_mut(circle_id).unwrap();
        if ok {
            c.dao_ready = true;
            emit("dao_ready", json!({"circle_id": circle_id, "dao": c.dao}));
            self.try_activate(circle_id);
        } else {
            log!("DAO for circle {} not found yet; call confirm_dao later", circle_id);
        }
        ok
    }

    /// Accept an invitation. Checks the circle's minimum reputation.
    pub fn join(&mut self, circle_id: u32) {
        let who = env::predecessor_account_id();
        let score = self.score_of(&who);
        let c = self.circles.get_mut(circle_id).unwrap_or_else(|| env::panic_str("No such circle"));
        require!(c.status == CircleStatus::Forming, "Circle is not forming");
        let i = c.index_of(&who).unwrap_or_else(|| env::panic_str("You are not invited to this circle"));
        require!(!c.joined[i], "Already joined");
        require!(score >= c.min_reputation, "Your reputation is below this circle's minimum");
        c.joined[i] = true;
        emit("member_joined", json!({"circle_id": circle_id, "member": who, "score": score}));
        self.try_activate(circle_id);
    }

    fn try_activate(&mut self, circle_id: u32) {
        let c = self.circles.get_mut(circle_id).unwrap();
        if c.status == CircleStatus::Forming && c.dao_ready && c.joined.iter().all(|j| *j) {
            c.status = CircleStatus::Active;
            c.start_ns = U64(env::block_timestamp());
            emit(
                "circle_started",
                json!({"circle_id": circle_id, "first_beneficiary": c.members[0], "deadline_ns": U64(c.deadline_ns())}),
            );
        }
    }

    /// Pay this round's contribution (attach exactly `contribution`). The money goes straight to
    /// the circle's DAO. If you are the last to pay, the pot is released in the same transaction
    /// (attach ~300 Tgas).
    #[payable]
    pub fn contribute(&mut self, circle_id: u32) {
        let who = env::predecessor_account_id();
        let now = env::block_timestamp();
        let amount = env::attached_deposit();
        let (round, on_time, dao, settled) = {
            let c = self.circles.get_mut(circle_id).unwrap_or_else(|| env::panic_str("No such circle"));
            require!(c.status == CircleStatus::Active, "Circle is not active");
            require!(!c.payout_pending, "This round's payout is being processed, try again shortly");
            let i = c.index_of(&who).unwrap_or_else(|| env::panic_str("Not a member"));
            require!(!c.paid[i] && !c.covered[i], "Already settled for this round");
            require!(amount.as_yoctonear() == c.contribution.0, "Attach exactly the contribution amount");
            c.paid[i] = true;
            c.collected = U128(c.collected.0 + amount.as_yoctonear());
            (c.round, now <= c.deadline_ns(), c.dao.clone(), c.round_settled())
        };
        let (kind, pts) = if on_time {
            (RepKind::OnTime, SCORE_ON_TIME)
        } else {
            (RepKind::Late, SCORE_LATE)
        };
        self.add_rep(&who, kind.clone(), circle_id, round, pts);
        emit(
            "contribution",
            json!({"circle_id": circle_id, "round": round, "member": who, "amount": U128(amount.as_yoctonear()), "on_time": on_time}),
        );
        let transfer = Promise::new(dao).transfer(amount);
        if settled {
            self.start_payout(circle_id, Some(transfer));
        } else {
            transfer.detach();
        }
    }

    /// Releases the pot if the round is settled but the payout has not happened
    /// (e.g. after the members voted to cover a default, or a failed attempt). Anyone can call.
    pub fn release(&mut self, circle_id: u32) {
        let c = self.circle(circle_id);
        require!(c.status == CircleStatus::Active, "Circle is not active");
        require!(!c.payout_pending, "Payout already in progress");
        require!(c.round_settled(), "Not everyone has paid this round");
        self.start_payout(circle_id, None);
    }

    fn start_payout(&mut self, circle_id: u32, transfer: Option<Promise>) {
        let c = self.circles.get_mut(circle_id).unwrap();
        c.payout_pending = true;
        let beneficiary = c.members[c.round as usize].clone();
        let amount = U128(c.pot());
        let round = c.round;
        let kind = json!({"Transfer": {"token_id": "", "receiver_id": beneficiary, "amount": amount}});
        let args = json!({"proposal": {
            "description": format!("Njangi '{}' round {}: pot to {}", c.name, round + 1, beneficiary),
            "kind": kind
        }});
        let base = transfer.unwrap_or_else(|| Promise::new(c.dao.clone()));
        base.function_call(
            "add_proposal",
            serde_json::to_vec(&args).unwrap(),
            NearToken::from_yoctonear(0),
            GAS_ADD_PROPOSAL,
        )
        .then(
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_ON_PROPOSED)
                .on_payout_proposed(circle_id, round, beneficiary, amount),
        )
        .detach();
    }

    #[private]
    pub fn on_payout_proposed(
        &mut self,
        circle_id: u32,
        round: u32,
        beneficiary: AccountId,
        amount: U128,
        #[callback_result] proposal_id: Result<u64, PromiseError>,
    ) {
        let c = self.circles.get_mut(circle_id).unwrap();
        let Ok(pid) = proposal_id else {
            c.payout_pending = false;
            log!("Could not file payout proposal for circle {} round {}", circle_id, round);
            return;
        };
        let args = json!({
            "id": pid,
            "action": "VoteApprove",
            "proposal": {"Transfer": {"token_id": "", "receiver_id": beneficiary, "amount": amount}},
            "memo": "njangi keeper: every member has paid"
        });
        Promise::new(c.dao.clone())
            .function_call(
                "act_proposal",
                serde_json::to_vec(&args).unwrap(),
                NearToken::from_yoctonear(0),
                GAS_ACT_PROPOSAL,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_PAID)
                    .on_payout_done(circle_id, round, beneficiary, amount, U64(pid)),
            )
            .detach();
    }

    #[private]
    pub fn on_payout_done(
        &mut self,
        circle_id: u32,
        round: u32,
        beneficiary: AccountId,
        amount: U128,
        proposal_id: U64,
        #[callback_result] res: Result<(), PromiseError>,
    ) -> bool {
        let now = env::block_timestamp();
        let c = self.circles.get_mut(circle_id).unwrap();
        c.payout_pending = false;
        if res.is_err() || c.round != round {
            log!("Payout approval failed for circle {} round {}; call release to retry", circle_id, round);
            return false;
        }
        c.payouts.push(Payout {
            round,
            beneficiary: beneficiary.clone(),
            amount,
            proposal_id,
            timestamp_ns: U64(now),
        });
        c.round += 1;
        let n = c.members.len();
        c.paid = vec![false; n];
        c.covered = vec![false; n];
        let finished = c.round as usize == n;
        let members = c.members.clone();
        let dao = c.dao.clone();
        if finished {
            c.status = CircleStatus::Completed;
        }
        emit(
            "payout",
            json!({"circle_id": circle_id, "round": round, "beneficiary": beneficiary, "amount": amount, "dao": dao, "proposal_id": proposal_id}),
        );
        self.add_rep(&beneficiary, RepKind::Received, circle_id, round, 0);
        if finished {
            for m in &members {
                self.add_rep(m, RepKind::Completed, circle_id, round, SCORE_COMPLETED);
            }
            emit("circle_completed", json!({"circle_id": circle_id}));
        }
        true
    }

    // ------------------------------------------------------- DAO-only (member votes)

    /// Members voted to swap two future turns (e.g. someone has an emergency).
    /// Call through a FunctionCall proposal on the circle's DAO.
    pub fn dao_swap_turns(&mut self, circle_id: u32, a: u32, b: u32) {
        let c = self.circles.get_mut(circle_id).unwrap_or_else(|| env::panic_str("No such circle"));
        require!(env::predecessor_account_id() == c.dao, "Only the circle's DAO");
        require!(c.status != CircleStatus::Completed, "Circle finished");
        require!(!c.payout_pending, "Payout in progress");
        let (a, b) = (a as usize, b as usize);
        require!(a < c.members.len() && b < c.members.len() && a != b, "Bad turn index");
        require!(a >= c.round as usize && b >= c.round as usize, "Only turns that have not been paid out");
        c.members.swap(a, b);
        c.joined.swap(a, b);
        c.paid.swap(a, b);
        c.covered.swap(a, b);
        emit(
            "turns_swapped",
            json!({"circle_id": circle_id, "turn_a": a, "turn_b": b, "now_a": c.members[a], "now_b": c.members[b]}),
        );
    }

    /// Members voted to cover an overdue member's share from the social fund already in the DAO.
    /// Records a default against that member. Then anyone can `release`.
    pub fn dao_cover_default(&mut self, circle_id: u32, member: AccountId) {
        let (round, settled) = {
            let c = self.circles.get_mut(circle_id).unwrap_or_else(|| env::panic_str("No such circle"));
            require!(env::predecessor_account_id() == c.dao, "Only the circle's DAO");
            require!(c.status == CircleStatus::Active, "Circle is not active");
            require!(env::block_timestamp() > c.deadline_ns(), "Round is not overdue yet");
            let i = c.index_of(&member).unwrap_or_else(|| env::panic_str("Not a member"));
            require!(!c.paid[i] && !c.covered[i], "Member already settled");
            c.covered[i] = true;
            (c.round, c.round_settled())
        };
        self.add_rep(&member, RepKind::Default, circle_id, round, SCORE_DEFAULT);
        emit("default_covered", json!({"circle_id": circle_id, "round": round, "member": member}));
        log!("settled={}", settled);
    }

    // ------------------------------------------------------------------ views

    pub fn get_circle(&self, circle_id: u32) -> Option<Circle> {
        self.circles.get(circle_id).cloned()
    }

    pub fn get_circles(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<Circle> {
        let from = from_index.unwrap_or(0);
        let limit = limit.unwrap_or(50).min(100);
        (from..self.circles.len().min(from.saturating_add(limit)))
            .filter_map(|i| self.circles.get(i).cloned())
            .collect()
    }

    pub fn get_circle_count(&self) -> u32 {
        self.circles.len()
    }

    pub fn get_member_circles(&self, account_id: AccountId) -> Vec<u32> {
        self.member_circles.get(&account_id).cloned().unwrap_or_default()
    }

    pub fn get_reputation(&self, account_id: AccountId) -> Reputation {
        self.reputation.get(&account_id).cloned().unwrap_or_default()
    }

    pub fn get_round(&self, circle_id: u32) -> RoundView {
        let c = self.circle(circle_id);
        let mut paid = vec![];
        let mut covered = vec![];
        let mut unpaid = vec![];
        for (i, m) in c.members.iter().enumerate() {
            if c.paid[i] {
                paid.push(m.clone())
            } else if c.covered[i] {
                covered.push(m.clone())
            } else {
                unpaid.push(m.clone())
            }
        }
        let active = c.status == CircleStatus::Active;
        RoundView {
            circle_id,
            round: c.round,
            beneficiary: c.members.get(c.round as usize).cloned().filter(|_| active),
            deadline_ns: U64(if active { c.deadline_ns() } else { 0 }),
            overdue: active && env::block_timestamp() > c.deadline_ns(),
            paid,
            covered,
            unpaid,
            pot: U128(c.pot()),
        }
    }

    pub fn get_config(&self) -> serde_json::Value {
        json!({"owner": self.owner, "factory": self.factory, "dao_deposit": U128(self.dao_deposit.as_yoctonear()),
               "scores": {"on_time": SCORE_ON_TIME, "late": SCORE_LATE, "default": SCORE_DEFAULT, "completed": SCORE_COMPLETED}})
    }

    // ------------------------------------------------------------------ internal

    fn circle(&self, id: u32) -> &Circle {
        self.circles.get(id).unwrap_or_else(|| env::panic_str("No such circle"))
    }

    fn score_of(&self, a: &AccountId) -> i64 {
        self.reputation.get(a).map_or(0, |r| r.score)
    }

    fn add_rep(&mut self, a: &AccountId, kind: RepKind, circle_id: u32, round: u32, points: i64) {
        let mut r = self.reputation.get(a).cloned().unwrap_or_default();
        match kind {
            RepKind::OnTime => r.on_time += 1,
            RepKind::Late => r.late += 1,
            RepKind::Default => r.defaults += 1,
            RepKind::Received => r.received += 1,
            RepKind::Completed => r.completed += 1,
        }
        r.score += points;
        r.history.push(RepEvent { kind, circle_id, round, points, timestamp_ns: U64(env::block_timestamp()) });
        if r.history.len() > MAX_HISTORY {
            r.history.remove(0);
        }
        self.reputation.insert(a.clone(), r);
    }
}

#[cfg(test)]
mod tests;
