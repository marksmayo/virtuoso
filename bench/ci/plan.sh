#!/bin/bash
# A bench workflow's matrix from its scenario groups (one a line, on stdin) and each group's
# minutes in the last full run ($1: "<minutes> <group>" lines). Longest first: GitHub starts a
# matrix's jobs in its order, and a long group started in a late wave keeps the run going with
# the other runners idle. A group with no minutes yet (a new flow) counts as the longest.
# With $2 and $3: the app flows (app/, tam/) of at most $2 minutes packed, up to three a job and
# $3 minutes together, into one job each (a job's setup costs minutes before its first run; the
# tools still take turns on each scenario, with a reset before every run and a warm-up each).
set -euo pipefail
minutes="$1"
short="${2:-0}"
cap="${3:-0}"
# minutes, packable (1 or 0), group: the longest first.
awk -v short="$short" '
  { sub(/\r$/, "") }
  NR == FNR { if (NF >= 2) m[$2] = $1; next }
  NF == 0 { next }
  {
    g = $1
    known = (g in m)
    t = known ? m[g] : 1000000
    pack = (short > 0 && known && t <= short && g ~ /^(app|tam)\//) ? 1 : 0
    printf "%d\t%d\t%s\n", t, pack, g
  }
' "$minutes" - | sort -s -t "$(printf '\t')" -k1,1nr |
# First fit, longest first: each packable group into the first job with room for it.
awk -F '\t' -v cap="$cap" '
  {
    if ($2 == 1) {
      for (i = 1; i <= n; i++) if (packs[i] && count[i] < 3 && total[i] + $1 <= cap) break
      if (i <= n) { total[i] += $1; count[i]++; jobs[i] = jobs[i] "," $3; next }
    }
    n++
    total[n] = $1; count[n] = 1; jobs[n] = $3; packs[n] = $2
  }
  END { for (i = 1; i <= n; i++) printf "%d\t%s\n", total[i], jobs[i] }
' | sort -s -t "$(printf '\t')" -k1,1nr | cut -f2
