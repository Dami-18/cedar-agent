#!/usr/bin/env bash

set -euo pipefail

cargo build --release --bin cedar-agent --bin generate_bench_data --bin core_bench

echo ""
echo "══════════════════════════════════════════════════════"
echo " Build complete."
echo "   ./target/release/cedar-agent"
echo "   ./target/release/generate_bench_data"
echo "   ./target/release/core_bench"
echo ""
echo "══════════════════════════════════════════════════════"