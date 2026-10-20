#!/bin/zsh
# F-007.34 / AC-10, scripted: does a mute capture stream come back by itself?
#
# Reproduces the failure of F-007 §8g, where ScreenCaptureKit keeps a stream
# alive but stops calling the delegate entirely — no frames, no idle frames,
# no rejects — so the pre-F-007.30 watchdog had nothing to count and the client
# stared at a frozen frame until the process was killed.
#
# The trigger measured in the field was a *second* process capturing the same
# display and then going away: eight seconds after it stopped its stream, the
# first process went quiet for good. This probe recreates exactly that, so the
# failure no longer has to be waited for.
#
# Needs no RDP client: capture starts in `main`, before `serve`.
# Grant Screen Recording to the terminal you run this from, or the server falls
# back to the placeholder source and measures nothing.
#
# Usage: zsh run.sh [path-to-mac-rdp-server]   (default: the one on PATH)

set -u
BIN=${1:-mac-rdp-server}
OUT=$(mktemp -d /tmp/f007-silent.XXXXXX)
LOG=$OUT/victim.log
INTRUDER_LOG=$OUT/intruder.log
PORT=33899
INTRUDER_PORT=33900
SETTLE=12
WATCH_FOR=90

echo "logs: $LOG"
echo "      $INTRUDER_LOG"

# The escape codes `tracing` emits sit between "received=" and the digits, so
# they come off before anything is parsed. Getting this wrong once made a probe
# report FAIL on a healthy server.
strip_ansi() { sed -E 's/\x1b\[[0-9;]*m//g' "$1"; }

received_count() {
  strip_ansi "$LOG" | grep "capture stats" | tail -1 | sed -E 's/.*received=([0-9]+).*/\1/'
}

wait_for_first_frame() {
  for _ in $(seq 1 60); do
    grep -q "first frame received" "$1" && return 0
    perl -e 'select(undef,undef,undef,0.5)'
  done
  return 1
}

# Keep the *display* awake: a display sleep is a different failure (§8e) and
# would confuse this measurement with that one.
caffeinate -d -t 240 &
CAF=$!

MAC_RDP_LOG=debug "$BIN" --port $PORT > "$LOG" 2>&1 &
VICTIM=$!
trap 'kill -9 $VICTIM $INTRUDER 2>/dev/null; kill $CAF 2>/dev/null' EXIT

if ! wait_for_first_frame "$LOG"; then
  echo "FAIL: no first frame — capture is not permitted, or the start path hung (F-007.22)"
  strip_ansi "$LOG" | tail -20
  exit 1
fi

perl -e "select(undef,undef,undef,$SETTLE)"
BEFORE=$(received_count)
echo "received before the intruder: $BEFORE"

echo "second capture process at $(date +%T)"
MAC_RDP_LOG=info "$BIN" --port $INTRUDER_PORT > "$INTRUDER_LOG" 2>&1 &
INTRUDER=$!
if ! wait_for_first_frame "$INTRUDER_LOG"; then
  echo "SKIP: the second process never captured a frame, so the trigger was never applied"
  strip_ansi "$INTRUDER_LOG" | tail -10
  exit 0
fi

# A clean stop, which is what the field incident was: the second process
# stopped its stream and exited normally. If this stops reproducing, a kill -9
# is the variant worth trying next.
echo "stopping it cleanly at $(date +%T)"
kill -INT $INTRUDER 2>/dev/null
wait $INTRUDER 2>/dev/null
INTRUDER=

echo "watching the first process for ${WATCH_FOR}s"
perl -e "select(undef,undef,undef,$WATCH_FOR)"

AFTER=$(received_count)
echo "received after:              $AFTER"
echo
echo "--- watchdog and recovery lines ---"
strip_ansi "$LOG" \
  | grep -iE "stopped calling us entirely|delivering no usable frames|stopped unexpectedly|capture recovered|recovery attempt failed" \
  || echo "(none)"
echo
echo "--- last stats line ---"
strip_ansi "$LOG" | grep "capture stats" | tail -1
echo

SILENT=$(strip_ansi "$LOG" | grep -c "stopped calling us entirely")
RECOVERED=$(strip_ansi "$LOG" | grep -c "capture recovered")

if [ "$SILENT" -gt 0 ]; then
  if [ "${AFTER:-0}" -gt "${BEFORE:-0}" ] && [ "$RECOVERED" -gt 0 ]; then
    echo "PASS: the stream went mute, the watchdog saw it, and frames resumed (F-007.30, AC-10)"
    echo "      the client half of AC-10 — picture back without reconnecting — needs a real client"
    exit 0
  fi
  echo "FAIL: silence was detected but frames did not resume (received stuck at $AFTER)"
  exit 1
fi

if [ "${AFTER:-0}" -gt "${BEFORE:-0}" ]; then
  echo "INCONCLUSIVE: capture never went quiet, so the failure was not triggered this time."
  echo "              Not a defect: the trigger is a race in ScreenCaptureKit, not a certainty."
  echo "              Try again, or try killing the second process with -9 instead of -INT."
  exit 0
fi

echo "FAIL: capture stopped ($BEFORE -> $AFTER) and nothing in the log noticed —"
echo "      this is exactly the §8g blind spot that F-007.30 was written to close."
exit 1
