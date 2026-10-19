#!/bin/zsh
# F-007 AC-3, scripted: does the picture come back after the display sleeps?
#
# Runs the server with the capture counters visible, forces a display sleep,
# wakes the display, and reports whether `received` started rising again.
# Needs no RDP client: capture is started in `main`, before `serve`, so the
# whole failure reproduces with nobody connected.
#
# Grant Screen Recording to the terminal you run this from, or the server
# falls back to the placeholder source and measures nothing.
#
# Usage: zsh run.sh [path-to-mac-rdp-server]   (default: the one on PATH)

set -u
BIN=${1:-mac-rdp-server}
OUT=$(mktemp -d /tmp/f007-ac3.XXXXXX)
LOG=$OUT/server.log
PORT=33899
SLEEP_FOR=25
WAKE_WAIT=40

echo "log: $LOG"

# Hold off *system* sleep so the measurement is not cut short; display sleep
# stays allowed, because that is the thing under test.
caffeinate -s -t 240 &
CAF=$!

# Targeted trace: the capture module only. A blanket `trace` would also log
# every keystroke, and this measurement does not need them.
MAC_RDP_LOG='debug,mac_rdp_server::capture::screen=trace' \
  "$BIN" --port $PORT > "$LOG" 2>&1 &
SRV=$!
trap 'kill -9 $SRV 2>/dev/null; kill $CAF 2>/dev/null' EXIT

for _ in $(seq 1 60); do
  grep -q "first frame received" "$LOG" && break
  perl -e 'select(undef,undef,undef,0.5)'
done
if ! grep -q "first frame received" "$LOG"; then
  echo "FAIL: no first frame — the start path itself hung (F-007.22) or capture is not permitted"
  sed -E 's/\x1b\[[0-9;]*m//g' "$LOG"
  exit 1
fi

perl -e 'select(undef,undef,undef,12)'
BEFORE=$(grep "capture stats" "$LOG" | tail -1 | sed -E 's/.*received=([0-9]+).*/\1/')
echo "received before sleep: $BEFORE"

echo "display sleep at $(date +%T)"
pmset displaysleepnow
perl -e "select(undef,undef,undef,$SLEEP_FOR)"

echo "wake at $(date +%T)"
caffeinate -u -t 2
perl -e "select(undef,undef,undef,$WAKE_WAIT)"

AFTER=$(grep "capture stats" "$LOG" | tail -1 | sed -E 's/.*received=([0-9]+).*/\1/')
echo "received ${WAKE_WAIT}s after wake: $AFTER"

echo
echo "--- recovery lines ---"
grep -iE "recovering|capture recovered|recovery attempt failed" "$LOG" \
  | sed -E 's/\x1b\[[0-9;]*m//g' || echo "(none)"
echo

# A stack sample names the blocking frame; without it, "frozen" is a guess.
sample $SRV 3 -file "$OUT/sample.txt" >/dev/null 2>&1 \
  && echo "stack sample: $OUT/sample.txt"

if [ "${AFTER:-0}" -gt "${BEFORE:-0}" ]; then
  echo "PASS: frames resumed after wake (AC-3)"
else
  echo "FAIL: received stuck at $AFTER — capture never came back (AC-3)"
  echo "      expect the sample to show start_capture parked in __psynch_cvwait"
  exit 1
fi
