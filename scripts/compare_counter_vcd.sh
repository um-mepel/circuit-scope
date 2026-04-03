#!/usr/bin/env bash
# Regenerate VCD with verilog_core (csverilog) and Icarus (counter_tb.v), then summarize differences.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURES="$ROOT/src-tauri/verilog-core/tests/codegen_fixtures"
CORE="$ROOT/src-tauri/verilog-core"

OURS="/tmp/vcd_ours_counter.vcd"
ICARUS="/tmp/vcd_icarus_tb.vcd"

command -v iverilog >/dev/null 2>&1 || {
  echo "error: iverilog not found (install Icarus Verilog)" >&2
  exit 1
}

echo "== verilog_core: csverilog → $OURS (top: counter from counter.v)"
(cd "$CORE" && cargo run -q --bin csverilog -- "$OURS" --explicit "$FIXTURES/counter.v")

echo ""
echo "== Icarus: counter_tb.v + counter.v → $ICARUS"
(cd "$FIXTURES" && iverilog -o /tmp/counter_cmp_sim counter_tb.v counter.v && vvp /tmp/counter_cmp_sim >/dev/null)
mv "$FIXTURES/counter_tb.vcd" "$ICARUS"

echo ""
echo "== File shape"
wc -l "$OURS" "$ICARUS"

echo ""
echo "== Header (first 14 lines)"
echo "--- verilog_core ---"
sed -n '1,14p' "$OURS"
echo "--- Icarus ---"
sed -n '1,14p' "$ICARUS"

echo ""
echo "== Count variable: first 8 (time + value line)"
echo "--- verilog_core ---"
awk '/^#[0-9]+$/ {t=$0} /^b.* #$/ {print t, $0}' "$OURS" | head -8
echo "--- Icarus ---"
awk '/^#[0-9]+$/ {t=$0} /^b.* #$/ {print t, $0}' "$ICARUS" | head -8

echo ""
echo "== Same timestamp, last 8 count samples (numeric interpretation differs by reset)"
echo "--- verilog_core ---"
awk '/^#[0-9]+$/ {t=substr($0,2)} /^b.* #$/ {print t, $1}' "$OURS" | tail -8
echo "--- Icarus ---"
awk '/^#[0-9]+$/ {t=substr($0,2)} /^b.* #$/ {print t, $1}' "$ICARUS" | tail -8

echo ""
echo "== Value sequence alignment"
# Skip our initial 'bx' and Icarus 'bx' + reset 'b0'; compare next 99 binary tokens.
if diff -q \
  <(awk '/^b.* #$/ {print $1}' "$OURS" | sed -n '2,100p') \
  <(awk '/^b.* #$/ {print $1}' "$ICARUS" | sed -n '3,101p') \
  >/dev/null
then
  echo "OK: 99-value run matches (ours lines 2–100 == icarus lines 3–101 after skipping bx / bx+b0)."
else
  echo "MISMATCH: unexpected difference in aligned slices."
  diff <(awk '/^b.* #$/ {print $1}' "$OURS" | sed -n '2,100p') \
    <(awk '/^b.* #$/ {print $1}' "$ICARUS" | sed -n '3,101p') || true
fi

echo ""
echo "== Extra tail"
echo "verilog_core last count line:"
awk '/^#[0-9]+$/ {t=$0} /^b.* #$/ {l=t " " $0} END{print l}' "$OURS"
echo "Icarus last count line:"
awk '/^#[0-9]+$/ {t=$0} /^b.* #$/ {l=t " " $0} END{print l}' "$ICARUS"

echo ""
echo "Notes:"
echo "  • verilog_core simulates flat 'counter' with implicit 0 init and rst=0; first posedge increments at 5 ns."
echo "  • counter_tb holds rst=1 through the first posedge (loads 0), so Icarus count lags by 1 at every time step."
echo "  • verilog_core emits x on undriven inputs in \$dumpvars at t=0; Icarus shows rst=1, clk=0 there."
echo "  • For time-aligned identical waves, simulate 'counter' only and use Icarus with initial count=0 (see prior comparison)."
