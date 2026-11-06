#!/bin/sh
# Deterministic instruction count of the W1 pyramid under valgrind (card 4-22).
#
#   tools/icount/icount.sh [runs] [rows] [steps]
#
# Runs `tools_physics_bench icount-workload` under `valgrind --tool=callgrind` `runs` times
# (default 3, at least 2; rows 20, steps 30), prints each instruction count and the spread between the runs
# with the verdict on a 5 % / 10 % gate. Linux only (valgrind). Build first:
#   cargo build --release -p tools_physics_bench
# Set BENCH to use another binary. Exit codes: 0 done, 2 valgrind or the binary is missing,
# 1 a run did not print a count.
#
# The valgrind command line is the one the hosted probe ran; the script itself was checked only
# with a stand-in valgrind (none on the author's machine).
# Measured on a GitHub-hosted ubuntu-26.04 runner (AMD EPYC 7763, valgrind 3.26.0): the counts of
# six runs in two separate workflow runs were identical. `perf stat -e instructions:u` is not
# supported there. See docs/budgets/physics-benchmark-4-22.md.

runs=${1:-3}
rows=${2:-20}
steps=${3:-30}
bench=${BENCH:-target/release/tools_physics_bench}

if [ "$runs" -lt 2 ]; then
  echo "runs must be at least 2 (a spread needs two counts)" >&2
  exit 2
fi
if ! command -v valgrind >/dev/null 2>&1; then
  echo "valgrind not found (install it: sudo apt-get install valgrind)" >&2
  exit 2
fi
if [ ! -x "$bench" ]; then
  echo "$bench not found: build with cargo build --release -p tools_physics_bench" >&2
  exit 2
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
counts=""
i=1
while [ "$i" -le "$runs" ]; do
  valgrind --tool=callgrind --callgrind-out-file=/dev/null \
    "$bench" icount-workload --rows "$rows" --steps "$steps" >/dev/null 2>"$work/run$i.txt" || {
      echo "run $i: valgrind or the workload failed" >&2
      exit 1
    }
  count=$("$bench" icount-parse callgrind "$work/run$i.txt") || {
    echo "run $i printed no instruction count" >&2
    exit 1
  }
  echo "run $i: $count instructions"
  counts="$counts $count"
  i=$((i + 1))
done
# shellcheck disable=SC2086
"$bench" icount-spread $counts
