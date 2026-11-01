import "./polyfill";
import "./style.css";
import { CONTRACT, EXPLORER, accountId, balance, call, initWallet, near, nsToDate, signIn, signOut, toYocto, view } from "./near";
import type { Circle, Proposal, Reputation, RoundView } from "./types";

const app = document.getElementById("app")!;
const esc = (s: unknown) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const short = (a: string) => a.replace(/\.testnet$/, "");
const initial = (a: string) => (short(a).split(/[-_.]/).filter(Boolean).pop() ?? "?")[0].toUpperCase();
const acct = (a: string) => `<a class="acct" href="#/member/${esc(a)}"><span class="av" style="--h:${hue(a)}">${esc(initial(a))}</span>${esc(short(a))}</a>`;
const tx = (a: string) => `${EXPLORER}/address/${a}`;
function hue(s: string) { let h = 0; for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360; return h; }
function toast(msg: string, bad = false) {
  const t = document.getElementById("toast")!; t.textContent = msg; t.className = bad ? "show bad" : "show";
  setTimeout(() => (t.className = ""), 5000);
}
function ago(d: Date) {
  const s = (Date.now() - d.getTime()) / 1000, f = Math.abs(s);
  const u = f < 90 ? `${Math.round(f)}s` : f < 5400 ? `${Math.round(f / 60)} min` : f < 172800 ? `${Math.round(f / 3600)} h` : `${Math.round(f / 86400)} days`;
  return s >= 0 ? `${u} ago` : `in ${u}`;
}
function period(secs: number) {
  if (secs % 604800 === 0) return `${secs / 604800} week${secs > 604800 ? "s" : ""}`;
  if (secs % 86400 === 0) return `${secs / 86400} day${secs > 86400 ? "s" : ""}`;
  if (secs % 3600 === 0) return `${secs / 3600} h`;
  return `${Math.round(secs / 60)} min`;
}
function tier(score: number) {
  if (score >= 100) return ["Pillar", "gold"];
  if (score >= 40) return ["Trusted", "green"];
  if (score >= 0) return ["New", "sky"];
  return ["Watch", "red"];
}
async function act(label: string, fn: () => Promise<unknown>) {
  if (!accountId()) return signIn();
  try { toast(`${label}…`); await fn(); toast(`${label}: done`); route(); }
  catch (e: any) { toast(`${label} failed: ${String(e?.message ?? e).slice(0, 160)}`, true); }
}

// ---------------------------------------------------------------- views
function ring(c: Circle, size = 132) {
  const n = c.members.length, r = size / 2 - 14, cx = size / 2;
  const dots = c.members.map((m, i) => {
    const a = (i / n) * Math.PI * 2 - Math.PI / 2;
    const x = cx + r * Math.cos(a), y = cx + r * Math.sin(a);
    const got = c.payouts.some((p) => p.beneficiary === m);
    const cur = c.status === "Active" && i === c.round;
    const cls = cur ? "cur" : got ? "got" : c.paid[i] || c.covered[i] ? "paid" : "";
    return `<circle class="dot ${cls}" cx="${x}" cy="${y}" r="${cur ? 11 : 8}"><title>${esc(m)}</title></circle>`;
  }).join("");
  const pct = c.status === "Completed" ? 1 : c.payouts.length / n;
  const C = 2 * Math.PI * r;
  return `<svg class="ring" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}">
    <circle cx="${cx}" cy="${cx}" r="${r}" class="track"/>
    <circle cx="${cx}" cy="${cx}" r="${r}" class="prog" stroke-dasharray="${C * pct} ${C}" transform="rotate(-90 ${cx} ${cx})"/>
    ${dots}<text x="${cx}" y="${cx + 6}" text-anchor="middle" class="ringtxt">${c.payouts.length}/${n}</text></svg>`;
}

async function home() {
  const count = await view<number>("get_circle_count");
  const circles = (await view<Circle[]>("get_circles", { from_index: 0, limit: 50 })).reverse();
  const totalPaid = circles.reduce((s, c) => s + c.payouts.reduce((t, p) => t + BigInt(p.amount), 0n), 0n);
  const members = new Set(circles.flatMap((c) => c.members)).size;
  app.innerHTML = `
  <section class="hero">
    <div>
      <p class="kicker">Social money · NEAR testnet</p>
      <h1>The savings circle<br/><em>no treasurer</em> can run off with.</h1>
      <p class="lede">Across Cameroon, a <b>njangi</b> is how people save: everyone pays in each month, and one member takes the whole pot, turn by turn.
      It runs on trust in one treasurer. Here each circle's money sits in its <b>own Sputnik DAO</b>. The contract pays the pot out the moment everyone has paid,
      and the members vote on everything else.</p>
      <div class="cta"><a class="btn" href="#/new">Start a circle</a><a class="btn ghost" href="#/how">How it works</a></div>
    </div>
    <div class="stats">
      <div class="stat y"><b>${count}</b><span>circles</span></div>
      <div class="stat g"><b>${members}</b><span>members</span></div>
      <div class="stat r"><b>${near(totalPaid)} Ⓝ</b><span>paid out automatically</span></div>
    </div>
  </section>
  <h2 class="sec">Circles</h2>
  <div class="grid">${circles.map(card).join("") || `<p class="muted">No circles yet. <a href="#/new">Start the first one.</a></p>`}</div>`;
}

function card(c: Circle) {
  const pot = BigInt(c.contribution) * BigInt(c.members.length);
  const next = c.status === "Active" ? c.members[c.round] : null;
  return `<a class="card" href="#/circle/${c.id}">
    ${ring(c, 112)}
    <div class="cbody">
      <span class="pill ${c.status.toLowerCase()}">${c.status}</span>
      <h3>${esc(c.name)}</h3>
      <p>${near(c.contribution)} Ⓝ every ${period(+c.period_secs)} · pot ${near(pot)} Ⓝ</p>
      <p class="muted">${next ? `Round ${c.round + 1}: <b>${esc(short(next))}</b> collects` : c.status === "Forming" ? `${c.joined.filter(Boolean).length}/${c.members.length} joined` : "All rounds paid out"}</p>
    </div></a>`;
}

async function circlePage(id: number) {
  const [c, r] = await Promise.all([view<Circle>("get_circle", { circle_id: id }), view<RoundView>("get_round", { circle_id: id })]);
  if (!c) { app.innerHTML = `<p>Circle not found.</p>`; return; }
  const me = accountId();
  const myIdx = me ? c.members.indexOf(me) : -1;
  const [daoBal, proposals, reps] = await Promise.all([
    balance(c.dao).catch(() => 0n),
    view<Proposal[]>("get_proposals", { from_index: 0, limit: 100 }, c.dao).catch(() => [] as Proposal[]),
    Promise.all(c.members.map((m) => view<Reputation>("get_reputation", { account_id: m }))),
  ]);
  const fund = BigInt(c.collected) - c.payouts.reduce((s, p) => s + BigInt(p.amount), 0n);
  const dl = c.status === "Active" ? nsToDate(r.deadline_ns) : null;
  let action = "";
  if (myIdx >= 0 && c.status === "Forming" && !c.joined[myIdx]) action = `<button class="btn" data-act="join">Join this circle</button>`;
  else if (myIdx >= 0 && c.status === "Active" && !c.paid[myIdx] && !c.covered[myIdx]) action = `<button class="btn" data-act="pay">Pay ${near(c.contribution)} Ⓝ for round ${c.round + 1}</button>`;
  else if (c.status === "Active" && r.unpaid.length === 0) action = `<button class="btn" data-act="release">Release the pot</button>`;
  else if (myIdx >= 0 && c.status === "Active") action = `<span class="ok">✓ You're settled for this round</span>`;

  app.innerHTML = `
  <a class="back" href="#/">← All circles</a>
  <section class="chead">
    ${ring(c, 168)}
    <div>
      <span class="pill ${c.status.toLowerCase()}">${c.status}</span>
      <h1>${esc(c.name)}</h1>
      <p class="lede">${c.members.length} members · ${near(c.contribution)} Ⓝ every ${period(+c.period_secs)} · ${c.fund_bps / 100}% of each pot goes to the social fund${c.min_reputation > 0 ? ` · needs reputation ≥ ${c.min_reputation}` : ""}</p>
      <p class="muted">Treasury: <a href="${tx(c.dao)}" target="_blank" rel="noopener">${esc(c.dao)}</a> · holds ${near(daoBal)} Ⓝ (incl. ~6 Ⓝ storage) ${c.dao_ready ? `<span class="ok">✓ keeper verified</span>` : `<span class="warn">DAO not verified yet</span>`}</p>
      <div class="cta">${action}</div>
    </div>
  </section>
  ${c.status === "Active" ? `
  <section class="round ${r.overdue ? "overdue" : ""}">
    <div><span class="kicker">Round ${c.round + 1} of ${c.members.length}</span>
      <h2>${acct(r.beneficiary!)} collects <b>${near(r.pot)} Ⓝ</b></h2>
      <p>${r.overdue ? `Deadline passed ${ago(dl!)}. Late payments still count (−15 reputation), or members can vote to cover a default from the social fund.` : `Deadline ${ago(dl!)} · ${dl!.toLocaleString()}`}</p></div>
    <div class="paybar">${c.members.map((m, i) => `<span class="${c.paid[i] ? "p" : c.covered[i] ? "c" : ""}" title="${esc(m)}"></span>`).join("")}</div>
    <p class="muted">${r.paid.length} paid · ${r.covered.length} covered · ${r.unpaid.length} to go${r.unpaid.length ? `: ${r.unpaid.map(short).map(esc).join(", ")}` : ""}</p>
  </section>` : ""}
  <div class="cols">
    <section class="panel">
      <h3>Turn order</h3>
      <ol class="order">${c.members.map((m, i) => {
        const p = c.payouts.find((x) => x.beneficiary === m);
        const [t, col] = tier(reps[i].score);
        return `<li class="${c.status === "Active" && i === c.round ? "now" : ""}">
          ${acct(m)} <span class="tier ${col}">${t} · ${reps[i].score}</span>
          <span class="right">${p ? `<span class="ok">received ${near(p.amount)} Ⓝ</span>` : c.status === "Forming" ? (c.joined[i] ? "joined" : `<span class="muted">invited</span>`) : c.status === "Active" && i === c.round ? "<b>collecting now</b>" : `<span class="muted">round ${i + 1}</span>`}</span></li>`;
      }).join("")}</ol>
      <p class="muted small">Social fund: ${near(fund > 0n ? fund : 0n)} Ⓝ kept in the DAO for emergencies and defaults.</p>
    </section>
    <section class="panel">
      <h3>Members decide</h3>
      <p class="muted small">Proposals on the circle's DAO. Payouts are proposed and approved by the contract (the <i>keeper</i> role, transfers only); everything else needs half the members.</p>
      ${myIdx >= 0 && c.status === "Active" ? `
      <details class="propose"><summary>+ New proposal</summary>
        <label>Swap turns <select id="sa">${c.members.map((m, i) => `<option value="${i}">${i + 1}. ${esc(short(m))}</option>`).join("")}</select> ↔
        <select id="sb">${c.members.map((m, i) => `<option value="${i}" ${i === 1 ? "selected" : ""}>${i + 1}. ${esc(short(m))}</option>`).join("")}</select></label>
        <input id="sreason" placeholder="Why? e.g. school fees are due" maxlength="140"/>
        <button class="btn small" data-act="swap">Propose swap</button>
        <hr/>
        <label>Cover a missed payment for <select id="cm">${r.unpaid.map((m) => `<option>${esc(m)}</option>`).join("")}</select></label>
        <button class="btn small ghost" data-act="cover" ${r.unpaid.length && r.overdue ? "" : "disabled"}>Propose cover from social fund</button>
      </details>` : ""}
      <ul class="props">${proposals.slice().reverse().map((p) => propRow(p, c, me)).join("") || `<li class="muted">No proposals yet.</li>`}</ul>
    </section>
  </div>`;

  app.querySelectorAll<HTMLButtonElement>("[data-act]").forEach((b) => b.onclick = () => {
    const a = b.dataset.act!;
    if (a === "join") act("Joining", () => call("join", { circle_id: id }));
    if (a === "pay") act("Paying", () => call("contribute", { circle_id: id }, c.contribution));
    if (a === "release") act("Releasing", () => call("release", { circle_id: id }));
    if (a === "swap") {
      const sa = +(document.getElementById("sa") as HTMLSelectElement).value, sb = +(document.getElementById("sb") as HTMLSelectElement).value;
      const why = (document.getElementById("sreason") as HTMLInputElement).value || "Swap turns";
      if (sa === sb) return toast("Pick two different turns", true);
      act("Proposing swap", () => daoFn(c, `Swap turns ${sa + 1} ↔ ${sb + 1}: ${why}`, "dao_swap_turns", { circle_id: id, a: sa, b: sb }));
    }
    if (a === "cover") {
      const m = (document.getElementById("cm") as HTMLSelectElement).value;
      act("Proposing cover", () => daoFn(c, `Cover ${m}'s missed payment for round ${c.round + 1} from the social fund`, "dao_cover_default", { circle_id: id, member: m }));
    }
    if (a === "vote") {
      const p = proposals.find((x) => x.id === +b.dataset.id!)!;
      act("Voting", () => call("act_proposal", { id: p.id, action: b.dataset.v, proposal: p.kind }, "0", 200, c.dao));
    }
  });
}

function daoFn(c: Circle, description: string, method: string, args: object) {
  const enc = btoa(JSON.stringify(args));
  return call("add_proposal", { proposal: { description, kind: { FunctionCall: { receiver_id: CONTRACT, actions: [{ method_name: method, args: enc, deposit: "0", gas: "50000000000000" }] } } } }, "0", 100, c.dao);
}

function propRow(p: Proposal, c: Circle, me: string | null) {
  const k = Object.keys(p.kind)[0];
  const isPayout = k === "Transfer";
  const title = isPayout ? `Pay ${near(p.kind.Transfer.amount)} Ⓝ to ${short(p.kind.Transfer.receiver_id)}` : p.description;
  const votes = Object.entries(p.votes);
  const canVote = me && c.members.includes(me) && p.status === "InProgress" && !p.votes[me];
  return `<li><span class="pill ${p.status === "Approved" ? "active" : p.status === "InProgress" ? "forming" : "completed"}">${p.status === "InProgress" ? "Voting" : p.status}</span>
    <div><b>${esc(title)}</b><br/><span class="muted small">#${p.id} by ${esc(short(p.proposer))}${isPayout ? " (keeper, automatic)" : ""} · ${votes.map(([a, v]) => `${esc(short(a))} ${v === "Approve" ? "✓" : "✗"}`).join(" ")}</span></div>
    ${canVote ? `<span class="right"><button class="btn small" data-act="vote" data-v="VoteApprove" data-id="${p.id}">Approve</button><button class="btn small ghost" data-act="vote" data-v="VoteReject" data-id="${p.id}">Reject</button></span>` : ""}</li>`;
}

async function memberPage(a: string) {
  const [rep, ids] = await Promise.all([view<Reputation>("get_reputation", { account_id: a }), view<number[]>("get_member_circles", { account_id: a })]);
  const circles = await Promise.all(ids.map((id) => view<Circle>("get_circle", { circle_id: id })));
  const [t, col] = tier(rep.score);
  const label: Record<string, string> = { OnTime: "Paid on time", Late: "Paid late", Default: "Missed a payment (covered)", Received: "Received the pot", Completed: "Completed a circle" };
  app.innerHTML = `
  <a class="back" href="#/">← All circles</a>
  <section class="chead">
    <span class="bigav" style="--h:${hue(a)}">${esc(initial(a))}</span>
    <div><span class="kicker">Reputation travels with you</span><h1>${esc(short(a))}</h1>
    <p><span class="tier big ${col}">${t} · ${rep.score} pts</span></p>
    <p class="muted">Circles can set a minimum reputation, so members with a good record get into bigger circles.</p></div>
  </section>
  <div class="stats row">
    <div class="stat g"><b>${rep.on_time}</b><span>on time (+10)</span></div>
    <div class="stat y"><b>${rep.late}</b><span>late (−15)</span></div>
    <div class="stat r"><b>${rep.defaults}</b><span>defaults (−50)</span></div>
    <div class="stat g"><b>${rep.completed}</b><span>circles completed (+20)</span></div>
  </div>
  <div class="cols">
    <section class="panel"><h3>History</h3><ul class="hist">${rep.history.slice().reverse().map((e) => `<li><span class="pts ${e.points > 0 ? "pos" : e.points < 0 ? "neg" : ""}">${e.points > 0 ? "+" : ""}${e.points}</span> <span>${label[e.kind]} · <a href="#/circle/${e.circle_id}">circle ${e.circle_id}</a>, round ${e.round + 1}</span> <span class="muted small right">${ago(nsToDate(e.timestamp_ns))}</span></li>`).join("") || `<li class="muted">No activity yet.</li>`}</ul></section>
    <section class="panel"><h3>Circles</h3><div class="grid one">${circles.map(card).join("") || `<p class="muted">None yet.</p>`}</div></section>
  </div>`;
}

function newPage() {
  const me = accountId();
  app.innerHTML = `
  <a class="back" href="#/">← All circles</a>
  <h1>Start a circle</h1>
  <p class="lede">Creating a circle also creates its Sputnik DAO treasury. That costs a <b>6 Ⓝ</b> storage deposit (free testnet NEAR), which stays in the DAO.</p>
  <form id="f" class="panel form">
    <label>Circle name<input name="name" required maxlength="64" placeholder="Bamenda Market Women"/></label>
    <label><span>Treasury name (becomes <code>&lt;name&gt;.sputnikv2.testnet</code>)</span><input name="dao" required pattern="[a-z0-9_\\-]{1,32}" placeholder="bamenda-market"/></label>
    <label>Members in turn order, one account per line (you must be one of them)<textarea name="members" rows="5" required>${me ? esc(me) + "\n" : ""}</textarea></label>
    <div class="row3">
      <label>Contribution (Ⓝ)<input name="amt" type="number" step="0.01" min="0.01" value="1"/></label>
      <label>Every<select name="period"><option value="300">5 minutes (demo)</option><option value="86400">day</option><option value="604800" selected>week</option><option value="2592000">30 days</option></select></label>
      <label>Social fund %<input name="fund" type="number" min="0" max="20" value="10"/></label>
    </div>
    <label>Minimum reputation to join<input name="minrep" type="number" value="0"/></label>
    <button class="btn">${me ? "Create circle (6 Ⓝ deposit)" : "Connect a wallet to create"}</button>
  </form>`;
  (document.getElementById("f") as HTMLFormElement).onsubmit = (ev) => {
    ev.preventDefault();
    const f = new FormData(ev.target as HTMLFormElement);
    const members = String(f.get("members")).split(/[\s,]+/).filter(Boolean);
    act("Creating circle", async () => {
      await call("create_circle", {
        name: f.get("name"), dao_name: f.get("dao"), members, contribution: toYocto(String(f.get("amt"))),
        period_secs: String(f.get("period")), fund_bps: Math.round(+f.get("fund")! * 100), min_reputation: +f.get("minrep")!,
      }, toYocto("6"));
      location.hash = "#/";
    });
  };
}

function howPage() {
  app.innerHTML = `
  <a class="back" href="#/">← All circles</a>
  <h1>How it works</h1>
  <div class="steps">
    <div class="step y"><b>1</b><h3>Start</h3><p>Someone lists the members in turn order and the amount. The contract creates a Sputnik DAO just for this circle. The contract is its <i>keeper</i> (allowed only to propose and approve payouts), and the members are its council.</p></div>
    <div class="step g"><b>2</b><h3>Pay in</h3><p>Every round each member pays the contribution. It goes straight into the DAO, never to a person. Paying on time earns +10 reputation; paying late costs 15.</p></div>
    <div class="step r"><b>3</b><h3>Pot out, automatically</h3><p>The moment the last member pays, the contract files a payout proposal on the DAO and approves it in the same transaction. The pot (minus the social-fund share) lands with that round's member.</p></div>
    <div class="step y"><b>4</b><h3>Members decide the rest</h3><p>Swap turns when someone has an emergency, or cover a missed payment from the social fund: any member proposes, and half the circle must approve on the DAO.</p></div>
  </div>
  <section class="panel"><h3>Why this is safe</h3>
  <ul class="bul"><li>No treasurer: the money sits in a DAO whose only automatic power is "pay the member whose turn it is, once everyone has paid".</li>
  <li>The contract checks the DAO's policy on chain before a circle can start (<code>keeper = this contract only</code>).</li>
  <li>Reputation is public and portable, so a member's record follows them to the next circle.</li>
  <li>Everything is on NEAR testnet and open source. Contract: <a href="${tx(CONTRACT)}" target="_blank" rel="noopener">${CONTRACT}</a>.</li></ul></section>`;
}

// ---------------------------------------------------------------- router + wallet
async function route() {
  const h = location.hash.replace(/^#\/?/, "");
  const [p, arg] = h.split("/");
  app.classList.add("loading");
  try {
    if (p === "circle") await circlePage(+arg);
    else if (p === "member") await memberPage(decodeURIComponent(arg));
    else if (p === "new") newPage();
    else if (p === "how") howPage();
    else await home();
  } catch (e: any) {
    app.innerHTML = `<p class="warn">Couldn't load from NEAR testnet: ${esc(e?.message ?? e)}</p>`;
  }
  app.classList.remove("loading");
  window.scrollTo({ top: 0 });
}

function renderWallet() {
  const b = document.getElementById("wallet") as HTMLButtonElement;
  const me = accountId();
  b.textContent = me ? `${short(me)} · sign out` : "Connect wallet";
  b.onclick = () => (me ? signOut() : signIn());
}

const cl = document.getElementById("contract-link") as HTMLAnchorElement;
cl.textContent = CONTRACT; cl.href = tx(CONTRACT);
window.addEventListener("hashchange", route);
route();
let last = "";
initWallet(() => { renderWallet(); const a = accountId() ?? ""; if (a !== last) { last = a; route(); } }).then(renderWallet).catch((e) => console.warn("wallet", e));
