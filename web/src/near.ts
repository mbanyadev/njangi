// Thin NEAR layer: read-only RPC views via fetch, writes via Wallet Selector.
import { setupWalletSelector, type WalletSelector } from "@near-wallet-selector/core";
import { setupModal, type WalletSelectorModal } from "@near-wallet-selector/modal-ui";
import { setupMyNearWallet } from "@near-wallet-selector/my-near-wallet";
import { setupMeteorWallet } from "@near-wallet-selector/meteor-wallet";
import "@near-wallet-selector/modal-ui/styles.css";
import { actionCreators } from "@near-js/transactions";

export const CONTRACT = "njangi-app.testnet";
export const NETWORK = "testnet";
export const RPC = "https://rpc.testnet.near.org";
export const EXPLORER = "https://testnet.nearblocks.io";
const RPCS = [RPC, "https://test.rpc.fastnear.com"];

let selector: WalletSelector;
let modal: WalletSelectorModal;

export async function initWallet(onChange: () => void) {
  selector = await setupWalletSelector({
    network: NETWORK,
    modules: [setupMyNearWallet(), setupMeteorWallet()],
  });
  modal = setupModal(selector, { contractId: CONTRACT, description: "Sign in to join and pay into circles." });
  selector.store.observable.subscribe(() => onChange());
}

export function accountId(): string | null {
  const s = selector?.store.getState();
  return s?.accounts.find((a) => a.active)?.accountId ?? null;
}
export function signIn() { modal.show(); }
export async function signOut() { const w = await selector.wallet(); await w.signOut(); }

async function rpc(method: string, params: unknown) {
  let last: unknown;
  for (const url of RPCS) {
    try {
      const r = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "njangi", method, params }),
      });
      const j = await r.json();
      if (j.error) throw new Error(j.error.data ?? j.error.message ?? JSON.stringify(j.error));
      if (j.result?.error) throw new Error(j.result.error);
      return j.result;
    } catch (e) { last = e; }
  }
  throw last;
}

const b64 = (s: string) => btoa(unescape(encodeURIComponent(s)));
export async function view<T>(method: string, args: object = {}, contract = CONTRACT): Promise<T> {
  const res = await rpc("query", {
    request_type: "call_function", finality: "optimistic",
    account_id: contract, method_name: method, args_base64: b64(JSON.stringify(args)),
  });
  return JSON.parse(new TextDecoder().decode(new Uint8Array(res.result)));
}

export async function balance(account: string): Promise<bigint> {
  const r = await rpc("query", { request_type: "view_account", finality: "optimistic", account_id: account });
  return BigInt(r.amount);
}

export async function call(method: string, args: object, depositYocto = "0", tgas = 300, receiver = CONTRACT) {
  const w = await selector.wallet();
  return w.signAndSendTransaction({
    receiverId: receiver,
    actions: [actionCreators.functionCall(method, args, BigInt(tgas) * 10n ** 12n, BigInt(depositYocto))],
  });
}

// ---------- formatting
const YOCTO = 10n ** 24n;
export function near(yocto: string | bigint, digits = 2): string {
  const v = BigInt(yocto);
  const whole = v / YOCTO;
  const frac = (v % YOCTO).toString().padStart(24, "0").slice(0, digits).replace(/0+$/, "");
  return frac ? `${whole}.${frac}` : `${whole}`;
}
export function toYocto(n: string): string {
  const [w, f = ""] = n.trim().split(".");
  return (BigInt(w || "0") * YOCTO + BigInt((f + "0".repeat(24)).slice(0, 24))).toString();
}
export const nsToDate = (ns: string | number) => new Date(Number(BigInt(ns) / 1_000_000n));
