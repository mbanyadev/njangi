#!/usr/bin/env bash
# Njangi end-to-end demo on NEAR testnet (5 members, 0.5 NEAR each, short rounds).
#   1. ama creates a circle -> a Sputnik DAO is created for it (6 NEAR storage deposit)
#   2. everyone joins -> the circle starts
#   3. round 1: everyone pays on time -> the contract proposes + approves the payout to ama
#   4. chi asks to swap turns with eko; 3 of 5 members approve on the DAO
#   5. round 2: ama pays after the deadline (late, -15) -> payout to bih
# Needs near-cli-rs and the five demo accounts in the legacy keychain (~/.near-credentials).
set -euo pipefail
C=${CONTRACT:-njangi-app.testnet}
PERIOD=${PERIOD:-120}
NET="network-config testnet"
call() { # signer receiver method json deposit
  near contract call-function as-transaction "$2" "$3" json-args "$4" prepaid-gas '300.0 Tgas' attached-deposit "$5" \
    sign-as "njangi-$1.testnet" $NET sign-with-legacy-keychain send 2>&1 | grep -E "Transaction ID|rror|panicked" | sed "s/^/   [$1 $3] /" || true
}
view() { near contract call-function as-read-only "$1" "$2" json-args "$3" $NET now 2>/dev/null; }
pay() { call "$1" "$C" contribute "{\"circle_id\":$ID}" '0.5 NEAR'; }
DAO=${DAO_NAME:-circle-$(date +%s)}
M='["njangi-ama.testnet","njangi-bih.testnet","njangi-chi.testnet","njangi-dze.testnet","njangi-eko.testnet"]'

echo "1) create circle + DAO $DAO.sputnikv2.testnet"
call ama "$C" create_circle "{\"name\":\"${NAME:-Bamenda Market Women}\",\"dao_name\":\"$DAO\",\"members\":$M,\"contribution\":\"500000000000000000000000\",\"period_secs\":\"$PERIOD\",\"fund_bps\":1000,\"min_reputation\":0}" '6 NEAR'
ID=$(( $(view "$C" get_circle_count '{}' | tr -dc 0-9) - 1 )); echo "   circle id $ID"

echo "2) members join"
for a in bih chi dze eko; do call $a "$C" join "{\"circle_id\":$ID}" '0 NEAR'; done

echo "3) round 1: all pay on time -> automatic payout to ama"
for a in ama bih chi dze eko; do pay $a; done

echo "4) chi proposes swapping turns 3 <-> 5; chi, eko and ama approve"
D="$DAO.sputnikv2.testnet"
ARGS=$(printf '{"circle_id":%s,"a":2,"b":4}' "$ID" | base64 -w0)
KIND="{\"FunctionCall\":{\"receiver_id\":\"$C\",\"actions\":[{\"method_name\":\"dao_swap_turns\",\"args\":\"$ARGS\",\"deposit\":\"0\",\"gas\":\"50000000000000\"}]}}"
call chi "$D" add_proposal "{\"proposal\":{\"description\":\"Swap turns 3 <-> 5: school fees are due\",\"kind\":$KIND}}" '0 NEAR'
PID=$(( $(view "$D" get_last_proposal_id '{}' | tr -dc 0-9) - 1 ))
for a in chi eko ama; do call $a "$D" act_proposal "{\"id\":$PID,\"action\":\"VoteApprove\",\"proposal\":$KIND}" '0 NEAR'; done

echo "5) round 2: bih, chi, dze, eko pay on time; ama pays after the deadline"
for a in bih chi dze eko; do pay $a; done
DL=$(view "$C" get_round "{\"circle_id\":$ID}" | python3 -c "import json,sys;print(int(json.load(sys.stdin)['deadline_ns'])//10**9)")
W=$(( DL - $(date +%s) + 5 )); if [ $W -gt 0 ]; then echo "   waiting ${W}s for the deadline"; sleep $W; fi
pay ama

echo "== result"
view "$C" get_circle "{\"circle_id\":$ID}" | python3 -c "import json,sys;c=json.load(sys.stdin);print('   order:',[m.split('.')[0] for m in c['members']]);[print('   payout round',p['round']+1,'->',p['beneficiary'],int(p['amount'])/1e24,'NEAR (proposal',p['proposal_id']+')') for p in c['payouts']]"
view "$C" get_reputation '{"account_id":"njangi-ama.testnet"}' | python3 -c "import json,sys;r=json.load(sys.stdin);print('   ama reputation',r['score'],'(late',r['late'],')')"
echo "   web: https://mbanyadev.github.io/njangi/#/circle/$ID"
