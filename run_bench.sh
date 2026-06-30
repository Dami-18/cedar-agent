#!/usr/bin/env bash
set -euo pipefail

HOST="127.0.0.1"
PORT="8180"
DURATION="40s"
CONN_MULTIPLIER="4"
RATE="1000"
RESULTS_DIR="benchmark/results"

while [[ $# -gt 0 ]]; do
    case $1 in
        --host)            HOST="$2";            shift 2 ;;
        --port)            PORT="$2";            shift 2 ;;
        --duration)        DURATION="$2";        shift 2 ;;
        --conn-multiplier) CONN_MULTIPLIER="$2";  shift 2 ;;
        --rate)            RATE="$2";             shift 2 ;;
        --results-dir)     RESULTS_DIR="$2";      shift 2 ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
done

BASE_URL="http://${HOST}:${PORT}"
POLTREE_URL="${BASE_URL}/v1/is_authorized/poltree"
STATELESS_URL="${BASE_URL}/v1/is_authorized"
REQUESTS_FILE="bench_data/requests.jsonl"

AGENT_PID_FILE="benchmark/cedar_agent.pid"
AGENT_LOG_FILE="benchmark/cedar_agent.log"

mkdir -p "$RESULTS_DIR"

for cmd in wrk curl; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "ERROR: $cmd not found"; exit 1
    fi
done

if [[ ! -x "./target/release/cedar-agent" ]] || [[ ! -x "./target/release/generate_bench_data" ]]; then
    echo "ERROR: release binaries not found. Run ./benchmark/setup.sh first."
    exit 1
fi

stop_agent() {
    if [[ -f "$AGENT_PID_FILE" ]]; then
        local pid
        pid=$(cat "$AGENT_PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid"
            for _ in $(seq 1 10); do
                sleep 0.5
                kill -0 "$pid" 2>/dev/null || break
            done
            kill -9 "$pid" 2>/dev/null || true
        fi
    fi
    rm -f "$AGENT_PID_FILE"
}

start_agent() {
    stop_agent

    ./target/release/cedar-agent \
        --schema   bench_data/schema.json \
        --data     bench_data/entities.json \
        --policies bench_data/policies.json \
        --port     "$PORT" \
        --log-level debug \
        > "$AGENT_LOG_FILE" 2>&1 &

    echo $! > "$AGENT_PID_FILE"
    local pid
    pid=$(cat "$AGENT_PID_FILE")

    local attempts=0
    until curl -sf -o /dev/null -X GET "${BASE_URL}/v1/" 2>/dev/null; do
        sleep 0.5
        attempts=$(( attempts + 1 ))
        if [[ $attempts -ge 40 ]]; then
            echo "ERROR: cedar-agent did not start within 20s."
            tail -20 "$AGENT_LOG_FILE"
            exit 1
        fi
    done
}

warmup_poltree() {
    local first_request
    first_request=$(head -n 1 "$REQUESTS_FILE")

    curl -sf -o /dev/null \
        -X POST \
        -H "Content-Type: application/json" \
        -d "$first_request" \
        "$POLTREE_URL" || true

    sleep 1
}

run_wrk() {
    local label="$1"
    local url="$2"
    local outfile="${RESULTS_DIR}/${label}.txt"

    CEDAR_REQUESTS_FILE="$REQUESTS_FILE" wrk \
        -t "$THREADS" \
        -c "$CONNECTIONS" \
        -d "$DURATION" \
        -R "$RATE" \
        --latency \
        -s benchmark/cedar.lua \
        "$url" 2>&1 | tee "$outfile"

    echo ""
}

run_both() {
    local prefix="$1"
    run_wrk "${prefix}_poltree"   "$POLTREE_URL"
    run_wrk "${prefix}_stateless" "$STATELESS_URL"
}

load_and_start() {
    ./target/release/generate_bench_data "$@"
    start_agent
    warmup_poltree
}

trap stop_agent EXIT

THREADS_LEVELS=(16)
POLICY_COUNTS=(10 100 500 1000)
ENTITY_TOTALS=(10000 50000 100000 500000)
ATTR_COUNTS=(1 5 10 20)

echo "Experiment 1: Policy scaling"
for n_pol in "${POLICY_COUNTS[@]}"; do
    load_and_start \
        --policies "$n_pol" --users 500 --documents 2000 \
        --requests 25000 --departments 8 --attributes-per-entity 3 \
        --seed 42 --out bench_data

    for THREADS in "${THREADS_LEVELS[@]}"; do
        CONNECTIONS=$(( THREADS * CONN_MULTIPLIER ))
        run_both "policy_${n_pol}_t${THREADS}"
    done
done

echo "Experiment 2: Entity scaling"
for n_ent in "${ENTITY_TOTALS[@]}"; do
    n_users=$(( n_ent / 5 ))
    n_docs=$(( n_ent - n_users ))

    load_and_start \
        --users "$n_users" --documents "$n_docs" \
        --policies 100 --requests 25000 \
        --departments 8 --attributes-per-entity 3 \
        --seed 42 --out bench_data

    for THREADS in "${THREADS_LEVELS[@]}"; do
        CONNECTIONS=$(( THREADS * CONN_MULTIPLIER ))
        run_both "entities_${n_ent}_t${THREADS}"
    done
done

echo "Experiment 3: Attribute scaling"
for n_attr in "${ATTR_COUNTS[@]}"; do
    load_and_start \
        --users 500 --documents 2000 \
        --policies 100 --requests 25000 \
        --departments 8 --attributes-per-entity "$n_attr" \
        --seed 42 --out bench_data

    for THREADS in "${THREADS_LEVELS[@]}"; do
        CONNECTIONS=$(( THREADS * CONN_MULTIPLIER ))
        run_both "attrs_${n_attr}_t${THREADS}"
    done
done

echo "Done. Results -> ${RESULTS_DIR}/"