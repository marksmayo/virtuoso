#!/usr/bin/env bash
# peak-procs.sh: which process sets a run's peak memory.
#
# Usage: bench/ci/peak-procs.sh <command...>
#
# Runs the command and, while it runs, samples `ps -o pid,ppid,rss,command` every 200 ms for
# the processes under it (the command and everything it started that is still its descendant).
# At the end it prints, to stderr, each process type's peak RSS: a Chrome process's `--type=`
# (renderer, gpu-process, utility; a utility with its `--utility-sub-type=`, e.g.
# utility:network.mojom.NetworkService), "browser" for Chrome's own process (no --type), or
# the program's name otherwise. Each row gives the largest single process of that type (the
# figure the bench's wait4 ru_maxrss takes on Linux and macOS is the largest single process it
# waited for), its pid and how many processes of that type were seen; the last line is the
# whole tree's largest total in one sample.
#
# Processes that leave the tree (re-parented to init, as an orphaned browser is) are not seen,
# nor is anything that lives under 200 ms between samples. Linux and macOS (bash 3.2, BSD ps).
# Exit status: the command's.

set -u

if [ "$#" -eq 0 ]; then
  echo "usage: $0 <command...>" >&2
  exit 2
fi

samples=$(mktemp)
trap 'rm -f "$samples"' EXIT

"$@" &
root=$!

n=0
while kill -0 "$root" 2>/dev/null; do
  # One line per process under the root: sample, type, pid, RSS in KB.
  ps -A -o pid=,ppid=,rss=,command= | awk -v root="$root" -v sample="$n" '
    function kind(cmd,   t, w, s) {
      if (match(cmd, /--type=[^ ]+/)) {
        t = substr(cmd, RSTART + 7, RLENGTH - 7)
        if (t == "utility" && match(cmd, /--utility-sub-type=[^ ]+/)) {
          t = t ":" substr(cmd, RSTART + 19, RLENGTH - 19)
        }
        return t
      }
      if (cmd ~ /chromedriver/) return "chromedriver"
      if (cmd ~ /[Cc]hrom(e|ium)/) return "browser"
      split(cmd, w, " ")
      s = w[1]
      sub(/.*\//, "", s)
      return s
    }
    {
      pid = $1
      parent[pid] = $2
      rss[pid] = $3
      cmd = $0
      sub(/^ *[0-9]+ +[0-9]+ +[0-9]+ */, "", cmd)
      command[pid] = cmd
      pids[count++] = pid
    }
    END {
      tree[root] = 1
      grew = 1
      while (grew) {
        grew = 0
        for (i = 0; i < count; i++) {
          p = pids[i]
          if (!(p in tree) && (parent[p] in tree)) {
            tree[p] = 1
            grew = 1
          }
        }
      }
      for (p in tree) {
        if (p in rss) print sample, kind(command[p]), p, rss[p]
      }
    }' >>"$samples"
  n=$((n + 1))
  sleep 0.2
done

wait "$root"
status=$?

{
  echo "peak RSS by process type ($n samples, 200 ms apart):"
  awk '
    {
      if (!(($2, $3) in seen)) {
        seen[$2, $3] = 1
        procs[$2]++
      }
      if ($4 > peak[$2]) {
        peak[$2] = $4
        at[$2] = $3
      }
      total[$1] += $4
    }
    END {
      for (t in peak) printf "  %-48s %9.1f MB  pid %-7s %d process(es)\n", t, peak[t] / 1024, at[t], procs[t]
      for (s in total) if (total[s] > most) most = total[s]
      printf "  %-48s %9.1f MB\n", "(whole tree, largest sample)", most / 1024
    }' "$samples" | sort -k2,2 -rn
} >&2

exit "$status"
