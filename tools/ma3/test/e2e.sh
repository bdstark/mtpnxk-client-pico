#!/bin/sh
# End-to-end check without onPC: the real plugin (stock Lua, stubbed console, behind udp_pipe_bridge.py)
# driven by the real Rust service. Needs: lua, python3, a release build of the service.
#   sh tools/ma3/test/e2e.sh
set -e
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
BIN="$ROOT/service/target/release/mtpnxk"
[ -x "$BIN" ] || (cd "$ROOT/service" && cargo build --release -q)
PORT=${PORT:-9817}
KEY=$("$BIN" keygen)
LOG="${TMPDIR:-/tmp}/mtpnxk-e2e-$$.log"
python3 "$ROOT/tools/ma3/test/udp_pipe_bridge.py" --port "$PORT" --plugin-arg "key=$KEY input=keyboard bench" --seconds 22 > "$LOG" 2>&1 &
BRIDGE=$!
sleep 1
fail=0
echo "--- scripted session (press, tap, release, unsupported key) ---"
OUT=$("$BIN" --plugin 127.0.0.1:$PORT --key "$KEY" sim --script "Record:down,5:tap@50,Record:up@300,Enter:tap,Clear:tap@200,Bank:tap@200" --seconds 3 2>&1 | grep -v '^led ')
echo "$OUT" | grep -q 'paired=true' || { echo "FAIL: not paired"; fail=1; }
echo "$OUT" | grep -q 'lost=0' || { echo "FAIL: events lost"; fail=1; }
echo "$OUT" | grep -q 'refused=2' || { echo "FAIL: expected the two Bank events refused"; fail=1; }
echo "$OUT" | grep -q 'rejected=0' || { echo "FAIL: plugin packets rejected"; fail=1; }
echo "--- abandoned session (no bye): the plugin's lease must release the held key ---"
"$BIN" --plugin 127.0.0.1:$PORT --key "$KEY" sim --script "Record:down" --seconds 1 --abandon > /dev/null 2>&1
sleep 3.5
grep -q 'lease expired, its holds were released' "$LOG" || { echo "FAIL: no lease expiry in the plugin log"; fail=1; }
echo "--- bench ---"
BENCH=$("$BIN" --plugin 127.0.0.1:$PORT --key "$KEY" bench --taps 100 --rate 20 2>&1)
echo "$BENCH" | grep -E 'ack round trip|press-to-effect|^link:'
echo "$BENCH" | grep -q 'lost=0' || { echo "FAIL: bench lost events"; fail=1; }
echo "$BENCH" | grep -q 'press-to-effect (NUM taps, command line): n=90' || { echo "FAIL: expected 90 press-to-effect samples"; fail=1; }
kill $BRIDGE 2>/dev/null || true
wait $BRIDGE 2>/dev/null || true
echo "--- plugin log excerpts ---"
grep -E 'session .* (opened|closed)|lease expired|released by heartbeat' "$LOG" | head -12
rm -f "$LOG"
[ $fail -eq 0 ] && echo "E2E PASSED" || { echo "E2E FAILED"; exit 1; }
