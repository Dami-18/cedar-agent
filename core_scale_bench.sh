#!/usr/bin/env bash
# Scaling benchmark for the in-process stateless-vs-poltree comparison
# (src/bin/core_bench.rs), for results-table / paper use.
#
# Unlike run_bench.sh (HTTP, wrk, one dataset), this sweeps four dimensions
# -- policy count, total entity count, attributes-per-entity, and department
# (attribute-value) cardinality -- regenerating the dataset and running
# core_bench fresh for each point, and writes one CSV row per point to
# benchmark/results/core_scale_bench.csv. Each row reports, separately:
#   - PolTree build cost (mean/min/max over --build-trials from-scratch
#     builds, bypassing the on-disk tree cache so it's real construction
#     cost, not deserialization) and the residual (fallback) policy count.
#   - Steady-state per-request latency/throughput for both backends, and the
#     latency/throughput speedup computed ONLY from that steady-state number
#     -- build cost is never folded into the speedup.
#
# The policy/entity/attribute sweep values and baselines mirror run_bench.sh
# so the two benchmarks' results are directly comparable. Two things worth
# knowing before reading the results:
#
#   - Attribute-count scaling (Experiment 3) varies `--attributes-per-entity`,
#     which adds extra entity attributes (attr_0, attr_1, ...) that NO policy
#     template ever references in a `when`-clause. PolTree's index build still
#     scans and indexes every entity attribute regardless of whether any
#     policy uses it, so this experiment is expected to show build time
#     growing with attribute count while query latency/throughput stay flat
#     -- it isolates build-cost sensitivity to entity shape, not query cost.
#
#   - "Attribute-value cardinality" (Experiment 4) sweeps `--departments`
#     specifically -- the attribute most heavily referenced by policy
#     conditions in this generator -- as a proxy for the cited N-PolTree
#     paper's |AV| factor (number of possible values per attribute, Tables
#     6/8 there). It is not a global measure across every attribute (other
#     attributes' value cardinalities, e.g. clearance's 4 levels, stay fixed);
#     say so explicitly if citing this alongside the paper's |AV| experiment.
set -euo pipefail

THREADS=16
ITERATIONS=20000
WARMUP_REQUESTS=10
BUILD_TRIALS=3
REQUESTS=25000
DEPARTMENTS=8
SEED=42
RESULTS_DIR="benchmark/results"
DATA_DIR="bench_data_scale_tmp"
QUICK=0

usage() {
    cat <<'USAGE'
Usage: ./core_scale_bench.sh [options]
  --threads N            wrk-style worker threads passed to core_bench (default 16)
  --iterations N         requests timed per backend per config (default 20000)
  --warmup-requests N    untimed warmup requests per backend per config (default 10)
  --build-trials N       independent from-scratch PolTree builds timed per config (default 3)
  --requests N           size of the generated request corpus per config (default 25000)
  --results-dir DIR      output directory (default benchmark/results)
  --quick                shrink the sweep + iteration counts for a fast smoke test

Runs 4 experiments (policy count, entity count, attributes-per-entity,
department/attribute-value cardinality), each varying one dimension while
holding the others at a fixed baseline, mirroring run_bench.sh's design.
USAGE
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --threads)          THREADS="$2";          shift 2 ;;
        --iterations)       ITERATIONS="$2";        shift 2 ;;
        --warmup-requests)  WARMUP_REQUESTS="$2";   shift 2 ;;
        --build-trials)     BUILD_TRIALS="$2";      shift 2 ;;
        --requests)         REQUESTS="$2";          shift 2 ;;
        --results-dir)      RESULTS_DIR="$2";       shift 2 ;;
        --quick)            QUICK=1;                shift 1 ;;
        -h|--help)          usage; exit 0 ;;
        *) echo "Unknown flag: $1" >&2; usage; exit 1 ;;
    esac
done

if [[ ! -x ./target/release/generate_bench_data ]] || [[ ! -x ./target/release/core_bench ]]; then
    echo "ERROR: release binaries not found. Run ./benchmark_setup.sh first." >&2
    exit 1
fi

if [[ "$QUICK" -eq 1 ]]; then
    ITERATIONS=2000
    WARMUP_REQUESTS=10
    BUILD_TRIALS=1
    REQUESTS=2000
    POLICY_COUNTS=(10 100)
    ENTITY_TOTALS=(1000 10000)
    ATTR_COUNTS=(1 10)
    DEPARTMENT_COUNTS=(2 8)
else
    POLICY_COUNTS=(10 100 500 1000)
    ENTITY_TOTALS=(10000 50000 100000 500000)
    ATTR_COUNTS=(1 5 10 20)
    DEPARTMENT_COUNTS=(2 4 8 16 32)
fi

mkdir -p "$RESULTS_DIR"
OUT_CSV="${RESULTS_DIR}/core_scale_bench.csv"

# A fresh PolTree cache dir avoids stale on-disk trees from previous sweeps
# accumulating -- harmless either way for correctness (the build-time metric
# always bypasses this cache), just disk hygiene.
rm -rf target/.cedar/poltree-cache

CORE_BENCH_HEADER=$(./target/release/core_bench --csv-header)
echo "experiment,varying_param,target_users,target_documents,target_policies,target_departments,target_attrs_per_entity,${CORE_BENCH_HEADER}" > "$OUT_CSV"

run_one() {
    local experiment="$1" varying="$2" users="$3" documents="$4" policies="$5" departments="$6" attrs="$7"

    rm -rf "$DATA_DIR"
    ./target/release/generate_bench_data \
        --users "$users" --documents "$documents" --policies "$policies" \
        --requests "$REQUESTS" --departments "$departments" --attributes-per-entity "$attrs" \
        --seed "$SEED" --out "$DATA_DIR" > /dev/null

    local row
    row=$(./target/release/core_bench \
        --data-dir "$DATA_DIR" \
        --threads "$THREADS" --iterations "$ITERATIONS" --warmup-requests "$WARMUP_REQUESTS" \
        --build-trials "$BUILD_TRIALS" --csv)

    echo "${experiment},${varying},${users},${documents},${policies},${departments},${attrs},${row}" >> "$OUT_CSV"
    echo "  [${experiment}] ${varying} -> done"
}

echo "Experiment 1: Policy scaling"
for n_pol in "${POLICY_COUNTS[@]}"; do
    run_one "policy_scaling" "policies=${n_pol}" 500 2000 "$n_pol" "$DEPARTMENTS" 3
done

echo "Experiment 2: Entity scaling"
for n_ent in "${ENTITY_TOTALS[@]}"; do
    n_users=$(( n_ent / 5 ))
    n_docs=$(( n_ent - n_users ))
    run_one "entity_scaling" "entities=${n_ent}" "$n_users" "$n_docs" 100 "$DEPARTMENTS" 3
done

echo "Experiment 3: Attribute scaling"
for n_attr in "${ATTR_COUNTS[@]}"; do
    run_one "attribute_scaling" "attrs_per_entity=${n_attr}" 500 2000 100 "$DEPARTMENTS" "$n_attr"
done

echo "Experiment 4: Attribute-value (department) cardinality scaling"
for n_dept in "${DEPARTMENT_COUNTS[@]}"; do
    run_one "attribute_value_scaling" "departments=${n_dept}" 500 2000 100 "$n_dept" 3
done

rm -rf "$DATA_DIR"
echo ""
echo "Done. Results -> ${OUT_CSV}"
