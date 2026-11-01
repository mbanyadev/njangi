export type Status = "Forming" | "Active" | "Completed";
export interface Payout { round: number; beneficiary: string; amount: string; proposal_id: string; timestamp_ns: string }
export interface Circle {
  id: number; name: string; creator: string; dao: string; members: string[]; joined: boolean[];
  contribution: string; period_secs: string; fund_bps: number; min_reputation: number; status: Status;
  dao_ready: boolean; created_ns: string; start_ns: string; round: number; paid: boolean[]; covered: boolean[];
  payout_pending: boolean; payouts: Payout[]; collected: string;
}
export interface RoundView { circle_id: number; round: number; beneficiary: string | null; deadline_ns: string; paid: string[]; covered: string[]; unpaid: string[]; pot: string; overdue: boolean }
export interface RepEvent { kind: "OnTime" | "Late" | "Default" | "Received" | "Completed"; circle_id: number; round: number; points: number; timestamp_ns: string }
export interface Reputation { score: number; on_time: number; late: number; defaults: number; received: number; completed: number; history: RepEvent[] }
export interface Proposal { id: number; proposer: string; description: string; kind: any; status: string; vote_counts: Record<string, [string, string, string]>; votes: Record<string, string>; submission_time: string }
