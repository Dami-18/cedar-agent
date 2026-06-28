#!/usr/bin/env bash
# benchmark/run_bench.sh

# cedar-agent runs 1 server with 2 routes:
#
#   POST /v1/is_authorized - stateless Cedar authorizer
#   POST /v1/is_authorized/poltree - PolTree cached authorizer
#
# Usage:
#   chmod +x benchmark/run_bench.sh
#   ./benchmark/run_bench.sh [OPTIONS]
#
# Options:
#   --host          HOST   cedar-agent hostname/IP   [127.0.0.1]
#   --port          PORT   cedar-agent port          [8180]
#   --time          SEC    sysbench run duration     [60]
#   --warmup        SEC    sysbench warmup duration  [10]
#   --results-dir   DIR    output directory          [benchmark/results]

#
# After completion, run:
#   python3 scripts/parse_sysbench.py
# to produce summary.csv and plots.

set -euo pipefail

HOST="127.0.0.1"
PORT="8180"
SYSBENCH_TIME="60"
SYSBENCH_WARMUP="10"
RESULTS_DIR="benchmark/results"

while [[ $# -gt 0 ]]; do
    case $1 in
        --host)         HOST="$2";           shift 2 ;;
        --port)         PORT="$2";           shift 2 ;;
        --time)         SYSBENCH_TIME="$2";  shift 2 ;;
        --warmup)       SYSBENCH_WARMUP="$2"; shift 2 ;;
        --results-dir)  RESULTS_DIR="$2";    shift 2 ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
done

BASE_URL="http://${HOST}:${PORT}"
POLTREE_URL="${BASE_URL}/v1/is_authorized/poltree"
STATELESS_URL="${BASE_URL}/v1/is_authorized"

mkdir -p "$RESULTS_DIR"

if ! command -v sysbench &>/dev/null; then
    echo "ERROR: sysbench not found."
    echo "  Ubuntu/Debian:  sudo apt install sysbench"
    echo "  macOS:          brew install sysbench"
    exit 1
fi

echo "Building dataset generator"
cargo build --release --bin generate_bench_data

# Runs sysbench with the cedar.lua script against a single endpoint.
# All sysbench custom options MUST be passed as --key=value (not --key value)
# because they are processed by the Lua script's cmdline.options table,
# not by sysbench's own option parser.
#
# Args: <label> <url> <threads>
run_sysbench() {
    local label="$1"
    local url="$2"
    local threads="$3"
    local outfile="${RESULTS_DIR}/${label}.txt"

    printf "  %-60s threads=%-4s\n" "$label" "$threads"

    sysbench benchmark/cedar.lua \
        "--cedar-url=${url}" \
        "--requests-file=bench_data/requests.jsonl" \
        "--threads=${threads}" \
        "--time=${SYSBENCH_TIME}" \
        "--warmup=${SYSBENCH_WARMUP}" \
        --report-interval=10 \
        run 2>&1 | tee "$outfile"

    echo ""
}

run_both() {
    local prefix="$1"   # e.g. "policy_100"
    local threads="$2"
    run_sysbench "${prefix}_poltree_t${threads}"   "$POLTREE_URL"   "$threads"
    run_sysbench "${prefix}_stateless_t${threads}" "$STATELESS_URL" "$threads"
}

CONCURRENCY=(1 8 16 32 64 128)
POLICY_COUNTS=(10 50 100 250 500 1000)
ENTITY_TOTALS=(10000 50000 100000 500000 1000000)
ATTR_COUNTS=(1 3 5 10 15 20)

# ─────────────────────────────────────────────────────────────────────────────
# Experiment 1 — Policy scaling
# Fixed:    500 users, 2000 docs, 3 attrs, 50k requests
# Variable: num_policies
# ─────────────────────────────────────────────────────────────────────────────
echo "══════════════════════════════════════════════════════"
echo " Experiment 1: Policy scaling"
echo "══════════════════════════════════════════════════════"

for n_pol in "${POLICY_COUNTS[@]}"; do
    echo ""
    echo "── Generating dataset: policies=${n_pol}, users=500, docs=2000, requests=50000 ──"
    ./target/release/generate_bench_data \
        --policies        "$n_pol" \
        --users           500 \
        --documents       2000 \
        --requests        50000 \
        --departments     8 \
        --attributes-per-entity 3 \
        --seed            42 \
        --out             bench_data

    echo "   → Load bench_data/entities.json and bench_data/policies.cedar"
    echo "     into cedar-agent before proceeding (if not already loaded)."
    echo "   Press ENTER to continue, or Ctrl-C to abort."
    read -r

    for threads in "${CONCURRENCY[@]}"; do
        run_both "policy_${n_pol}" "$threads"
    done
done

# ─────────────────────────────────────────────────────────────────────────────
# Experiment 2 — Entity scaling
# Fixed:    100 policies, 3 attrs, 50k requests
# Variable: total entities (users = N/5, docs = 4N/5)
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════════"
echo " Experiment 2: Entity scaling"
echo "══════════════════════════════════════════════════════"

for n_ent in "${ENTITY_TOTALS[@]}"; do
    n_users=$(( n_ent / 5 ))
    n_docs=$(( n_ent - n_users ))

    echo ""
    echo "── Generating: total_entities=${n_ent} (users=${n_users}, docs=${n_docs}) ──"
    ./target/release/generate_bench_data \
        --users           "$n_users" \
        --documents       "$n_docs" \
        --policies        100 \
        --requests        50000 \
        --departments     8 \
        --attributes-per-entity 3 \
        --seed            42 \
        --out             bench_data

    echo "   → Reload bench_data/ into cedar-agent, then press ENTER."
    read -r

    for threads in "${CONCURRENCY[@]}"; do
        run_both "entities_${n_ent}" "$threads"
    done
done

# ─────────────────────────────────────────────────────────────────────────────
# Experiment 3 — Attribute scaling
# Fixed:    500 users, 2000 docs, 100 policies, 50k requests
# Variable: attributes_per_entity
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════════"
echo " Experiment 3: Attribute scaling"
echo "══════════════════════════════════════════════════════"

for n_attr in "${ATTR_COUNTS[@]}"; do
    echo ""
    echo "── Generating: attrs_per_entity=${n_attr} ──"
    ./target/release/generate_bench_data \
        --users           500 \
        --documents       2000 \
        --policies        100 \
        --requests        50000 \
        --departments     8 \
        --attributes-per-entity "$n_attr" \
        --seed            42 \
        --out             bench_data

    echo "   → Reload bench_data/ into cedar-agent, then press ENTER."
    read -r

    for threads in "${CONCURRENCY[@]}"; do
        run_both "attrs_${n_attr}" "$threads"
    done
done

echo ""
echo "══════════════════════════════════════════════════════"
echo " All experiments complete."
echo " Results: ${RESULTS_DIR}/"
echo ""
echo " Generate plots:"
echo "   python3 scripts/parse_sysbench.py --results-dir ${RESULTS_DIR}"
echo "══════════════════════════════════════════════════════"