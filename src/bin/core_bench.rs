use std::fs;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use cedar_policy::{
    Authorizer, CachedAuthorizer, Context, Decision, Entities, EntityUid, Policy, PolicyId,
    PolicySet, Request,
};
use clap::Parser;
use serde::Deserialize;
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(
    name = "core_bench",
    about = "Benchmark Cedar stateless vs poltree evaluation in-process"
)]
struct Cli {
    #[arg(long, default_value = "bench_data")]
    data_dir: String,

    #[arg(long, default_value_t = 16)]
    threads: usize,

    #[arg(long, default_value_t = 20_000)]
    iterations: usize,

    #[arg(long, default_value_t = 2)]
    warmup_requests: usize,

    /// Number of independent, from-scratch PolTree builds to time. Reported
    /// separately from the per-request latency/throughput numbers below and
    /// NEVER folded into the speedup calculation -- it's a one-time setup
    /// cost, not a per-request cost.
    #[arg(long, default_value_t = 3)]
    build_trials: usize,

    /// Print only the CSV data row (see `csv_header` for the matching header)
    /// instead of the human-readable report. For sweeping many dataset
    /// configurations into a results table.
    #[arg(long)]
    csv: bool,

    /// Print only the CSV header line for `--csv` output, then exit
    /// immediately without loading any dataset.
    #[arg(long)]
    csv_header: bool,
}

const CSV_HEADER: &str = "policies,requests,entities,threads,iterations,warmup_requests,\
build_trials,build_mean_ms,build_min_ms,build_max_ms,residual_policies,total_policies,\
stateless_rps,stateless_avg_ms,stateless_p50_ms,stateless_p95_ms,stateless_p99_ms,stateless_max_ms,\
poltree_rps,poltree_avg_ms,poltree_p50_ms,poltree_p95_ms,poltree_p99_ms,poltree_max_ms,\
latency_speedup,throughput_speedup";

#[derive(Debug, Deserialize)]
struct PolicyEntry {
    id: String,
    content: String,
}

#[derive(Debug)]
struct BenchmarkSummary {
    name: String,
    threads: usize,
    iterations: usize,
    warmup_requests: usize,
    elapsed_ms: f64,
    throughput_rps: f64,
    avg_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    checksum: u64,
}

#[derive(Debug)]
struct WorkerResult {
    latencies_ms: Vec<f64>,
    checksum: u64,
}

/// PolTree construction cost, measured independently of any per-request
/// query. Always excluded from the speedup numbers -- it's a one-time,
/// amortized-over-many-requests setup cost, not a per-request cost -- but
/// reported so it can be cited on its own (e.g. "how long until this dataset's
/// tree is ready to serve").
#[derive(Debug)]
struct BuildStats {
    trials: usize,
    mean_ms: f64,
    min_ms: f64,
    max_ms: f64,
    /// Policies PolTree could not represent and must fall back to real
    /// evaluation for on every request (see `cedar_policy_core::poltree`).
    /// Non-zero here means the reported query speedup below is diluted by
    /// fallback overhead, not pure indexed-lookup performance.
    residual_policies: usize,
    total_policies: usize,
}

/// Time `trials` independent, from-scratch builds of the PolTree for
/// `policy_set`/`entities`, bypassing `CachedAuthorizer`'s disk cache
/// entirely so this measures real construction cost, not deserialization.
fn measure_poltree_build(
    policy_set: &PolicySet,
    entities: &Entities,
    trials: usize,
) -> BuildStats {
    let core_policy_set: &cedar_policy_core::ast::PolicySet = policy_set.as_ref();
    let core_entities: &cedar_policy_core::entities::Entities = entities.as_ref();

    let residual_ids =
        cedar_policy_core::poltree::residual_policy_ids(core_policy_set, core_entities);
    let total_policies = core_policy_set.policies().count();

    let mut times_ms = Vec::with_capacity(trials);
    for _ in 0..trials {
        let entities_arc = Arc::new(core_entities.clone());
        let start = Instant::now();
        let _tree = cedar_policy_core::poltree::PolTree::from_policy_set_and_entities(
            core_policy_set,
            entities_arc,
        );
        times_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let mean_ms = times_ms.iter().sum::<f64>() / times_ms.len() as f64;
    let min_ms = times_ms.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_ms = times_ms.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    BuildStats {
        trials,
        mean_ms,
        min_ms,
        max_ms,
        residual_policies: residual_ids.len(),
        total_policies,
    }
}

fn main() {
    let cli = Cli::parse();

    if cli.csv_header {
        println!("{CSV_HEADER}");
        return;
    }

    let data_dir = PathBuf::from(&cli.data_dir);
    let dataset = load_dataset(&data_dir).unwrap_or_else(|err| panic!("{err}"));

    let policy_set = Arc::new(
        build_policy_set(&dataset.policies_json)
            .unwrap_or_else(|err| panic!("failed to build policy set: {err}")),
    );
    let entities = Arc::new(
        build_entities(&dataset.entities_json)
            .unwrap_or_else(|err| panic!("failed to build entities: {err}")),
    );
    let requests = Arc::new(
        dataset
            .requests_json
            .iter()
            .map(|value| build_request(value))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|err| panic!("failed to build requests: {err}")),
    );

    if requests.is_empty() {
        panic!("benchmark request corpus is empty");
    }

    let build_stats = measure_poltree_build(&policy_set, &entities, cli.build_trials.max(1));

    let stateless_authorizer = Arc::new(Authorizer::new());
    let cached_authorizer = Arc::new(
        CachedAuthorizer::new_with_string_hash(
            (*policy_set).clone(),
            (*entities).clone(),
            &dataset.policies_raw,
            &dataset.entities_raw,
            None,
            None,
        )
        .unwrap_or_else(|err| panic!("failed to build cached authorizer: {err}")),
    );

    verify_backend_parity(
        &requests,
        &stateless_authorizer,
        &policy_set,
        &entities,
        &cached_authorizer,
    );

    let stateless_summary = run_benchmark(
        "stateless",
        &requests,
        cli.threads,
        cli.iterations,
        cli.warmup_requests,
        move |request| match stateless_authorizer.is_authorized(request, &policy_set, &entities).decision() {
            Decision::Allow => 1,
            Decision::Deny => 0,
        },
    );

    let cached_summary = run_benchmark(
        "poltree",
        &requests,
        cli.threads,
        cli.iterations,
        cli.warmup_requests,
        move |request| match cached_authorizer.is_authorized(request).decision() {
            Decision::Allow => 1,
            Decision::Deny => 0,
        },
    );

    if cli.csv {
        print_csv_row(&dataset, &build_stats, &stateless_summary, &cached_summary);
    } else {
        print_build_stats(&build_stats);
        print_summary(&stateless_summary);
        print_summary(&cached_summary);

        println!();
        println!(
            "latency speedup: {:.3}x  (build time above is excluded from this)",
            stateless_summary.avg_ms / cached_summary.avg_ms
        );
        println!(
            "throughput speedup: {:.3}x  (build time above is excluded from this)",
            cached_summary.throughput_rps / stateless_summary.throughput_rps
        );
    }
}

struct Dataset {
    policies_raw: String,
    entities_raw: String,
    policies_json: Value,
    entities_json: Value,
    requests_json: Vec<Value>,
}

fn load_dataset(data_dir: &PathBuf) -> Result<Dataset, String> {
    let policies_path = data_dir.join("policies.json");
    let entities_path = data_dir.join("entities.json");
    let requests_path = data_dir.join("requests.jsonl");

    let policies_raw = fs::read_to_string(&policies_path)
        .map_err(|err| format!("failed to read {}: {err}", policies_path.display()))?;
    let entities_raw = fs::read_to_string(&entities_path)
        .map_err(|err| format!("failed to read {}: {err}", entities_path.display()))?;
    let requests_raw = fs::read_to_string(&requests_path)
        .map_err(|err| format!("failed to read {}: {err}", requests_path.display()))?;

    let policies_json: Value = serde_json::from_str(&policies_raw)
        .map_err(|err| format!("failed to parse policies.json: {err}"))?;
    let entities_json: Value = serde_json::from_str(&entities_raw)
        .map_err(|err| format!("failed to parse entities.json: {err}"))?;
    let requests_json = requests_raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .map_err(|err| format!("failed to parse request line {line:?}: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Dataset {
        policies_raw,
        entities_raw,
        policies_json,
        entities_json,
        requests_json,
    })
}

fn build_policy_set(json: &Value) -> Result<PolicySet, String> {
    if let Some(obj) = json.as_object() {
        if obj.contains_key("staticPolicies")
            || obj.contains_key("templates")
            || obj.contains_key("templateLinks")
        {
            return PolicySet::from_json_value(json.clone())
                .map_err(|err| format!("failed to parse Cedar policy-set JSON: {err}"));
        }
    }

    let entries: Vec<PolicyEntry> = match json {
        Value::Array(entries) => entries
            .iter()
            .map(|entry| serde_json::from_value::<PolicyEntry>(entry.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("failed to parse benchmark policy entries: {err}"))?,
        Value::String(source) => {
            let policy = Policy::parse(None, source.clone())
                .map_err(|err| format!("failed to parse policy string: {err}"))?;
            return PolicySet::from_policies([policy])
                .map_err(|err| format!("failed to build policy set: {err}"));
        }
        _ => {
            return Err(
                "unsupported policies.json shape; expected Cedar policy-set JSON or benchmark array"
                    .to_string(),
            )
        }
    };

    let policies = entries
        .into_iter()
        .map(|entry| {
            let id = PolicyId::from_str(&entry.id)
                .map_err(|err| format!("invalid policy id {}: {err}", entry.id))?;
            Policy::parse(Some(id), entry.content)
                .map_err(|err| format!("failed to parse policy {}: {err}", entry.id))
        })
        .collect::<Result<Vec<_>, _>>()?;

    PolicySet::from_policies(policies).map_err(|err| format!("failed to build policy set: {err}"))
}

fn build_entities(json: &Value) -> Result<Entities, String> {
    Entities::from_json_value(json.clone(), None)
        .map_err(|err| format!("failed to parse entities JSON: {err}"))
}

fn build_request(json: &Value) -> Result<Request, String> {
    let principal = parse_entity_uid(json.get("principal"), "principal")?;
    let action = parse_entity_uid(json.get("action"), "action")?;
    let resource = parse_entity_uid(json.get("resource"), "resource")?;
    let context = match json.get("context") {
        Some(Value::Null) | None => Context::empty(),
        Some(value) => Context::from_json_value(value.clone(), None)
            .map_err(|err| format!("failed to parse request context: {err}"))?,
    };

    Request::new(principal, action, resource, context, None)
        .map_err(|err| format!("failed to build Cedar request: {err}"))
}

fn parse_entity_uid(value: Option<&Value>, field_name: &str) -> Result<EntityUid, String> {
    let value = value.ok_or_else(|| format!("missing {field_name} field in request"))?;

    match value {
        Value::String(uid) => EntityUid::from_str(uid)
            .map_err(|err| format!("failed to parse {field_name} `{uid}` as an entity UID: {err}")),
        _ => EntityUid::from_json(value.clone())
            .map_err(|err| format!("failed to parse {field_name} JSON as an entity UID: {err}")),
    }
}

fn verify_backend_parity(
    requests: &[Request],
    stateless_authorizer: &Authorizer,
    policy_set: &PolicySet,
    entities: &Entities,
    cached_authorizer: &CachedAuthorizer,
) {
    for request in requests.iter().take(128) {
        let stateless_decision = stateless_authorizer
            .is_authorized(request, policy_set, entities)
            .decision();
        let cached_decision = cached_authorizer.is_authorized(request).decision();

        if stateless_decision != cached_decision {
            panic!(
                "decision mismatch for request {:?}: stateless={:?}, poltree={:?}",
                request, stateless_decision, cached_decision
            );
        }
    }
}

fn run_benchmark<F>(
    name: &str,
    requests: &Arc<Vec<Request>>,
    threads: usize,
    iterations: usize,
    warmup_requests: usize,
    execute: F,
) -> BenchmarkSummary
where
    F: Fn(&Request) -> u64 + Send + Sync + 'static,
{
    assert!(threads > 0, "threads must be greater than zero");
    assert!(iterations > 0, "iterations must be greater than zero");

    let requests = Arc::clone(requests);
    let execute = Arc::new(execute);

    for warmup_idx in 0..warmup_requests {
        let request = &requests[warmup_idx % requests.len()];
        let _ = execute(request);
    }

    let start = Instant::now();
    let mut handles = Vec::with_capacity(threads);

    for thread_index in 0..threads {
        let requests = Arc::clone(&requests);
        let execute = Arc::clone(&execute);
        let start_index = thread_index * iterations / threads;
        let end_index = (thread_index + 1) * iterations / threads;

        handles.push(thread::spawn(move || {
            let mut latencies_ms = Vec::with_capacity(end_index - start_index);
            let mut checksum = 0_u64;

            for global_index in start_index..end_index {
                let request = &requests[global_index % requests.len()];
                let request_start = Instant::now();
                checksum = checksum.wrapping_add(execute(request));
                latencies_ms.push(request_start.elapsed().as_secs_f64() * 1000.0);
            }

            WorkerResult {
                latencies_ms,
                checksum,
            }
        }));
    }

    let mut latencies_ms = Vec::with_capacity(iterations);
    let mut checksum = 0_u64;

    for handle in handles {
        let worker = handle.join().expect("benchmark worker panicked");
        latencies_ms.extend(worker.latencies_ms);
        checksum = checksum.wrapping_add(worker.checksum);
    }

    let elapsed = start.elapsed();
    latencies_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    BenchmarkSummary {
        name: name.to_string(),
        threads,
        iterations,
        warmup_requests,
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        throughput_rps: iterations as f64 / elapsed.as_secs_f64(),
        avg_ms: average(&latencies_ms),
        p50_ms: percentile(&latencies_ms, 0.50),
        p95_ms: percentile(&latencies_ms, 0.95),
        p99_ms: percentile(&latencies_ms, 0.99),
        max_ms: latencies_ms.last().copied().unwrap_or(0.0),
        checksum,
    }
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    let rank = ((values.len() as f64) * percentile).ceil() as usize;
    let index = rank.saturating_sub(1).min(values.len() - 1);
    values[index]
}

fn print_build_stats(stats: &BuildStats) {
    println!();
    println!("=== poltree build (one-time cost; excluded from speedup below) ===");
    println!("trials             : {}", stats.trials);
    println!("mean ms            : {:.3}", stats.mean_ms);
    println!("min ms             : {:.3}", stats.min_ms);
    println!("max ms             : {:.3}", stats.max_ms);
    println!(
        "residual policies  : {} / {}{}",
        stats.residual_policies,
        stats.total_policies,
        if stats.residual_policies > 0 {
            "  <-- non-zero: query speedup below is diluted by real-evaluator fallback overhead, not pure PolTree lookup"
        } else {
            ""
        }
    );
}

fn print_csv_row(
    dataset: &Dataset,
    build: &BuildStats,
    stateless: &BenchmarkSummary,
    poltree: &BenchmarkSummary,
) {
    let policies = dataset.policies_json.as_array().map(Vec::len).unwrap_or(0);
    let entities = dataset.entities_json.as_array().map(Vec::len).unwrap_or(0);
    let requests = dataset.requests_json.len();

    println!(
        "{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
        policies,
        requests,
        entities,
        stateless.threads,
        stateless.iterations,
        stateless.warmup_requests,
        build.trials,
        build.mean_ms,
        build.min_ms,
        build.max_ms,
        build.residual_policies,
        build.total_policies,
        stateless.throughput_rps,
        stateless.avg_ms,
        stateless.p50_ms,
        stateless.p95_ms,
        stateless.p99_ms,
        stateless.max_ms,
        poltree.throughput_rps,
        poltree.avg_ms,
        poltree.p50_ms,
        poltree.p95_ms,
        poltree.p99_ms,
        poltree.max_ms,
        stateless.avg_ms / poltree.avg_ms,
        poltree.throughput_rps / stateless.throughput_rps,
    );
}

fn print_summary(summary: &BenchmarkSummary) {
    println!();
    println!("=== {} ===", summary.name);
    println!("threads         : {}", summary.threads);
    println!("iterations      : {}", summary.iterations);
    println!("warmup requests : {}", summary.warmup_requests);
    println!("elapsed ms      : {:.3}", summary.elapsed_ms);
    println!("throughput rps  : {:.3}", summary.throughput_rps);
    println!("avg ms          : {:.3}", summary.avg_ms);
    println!("p50 ms          : {:.3}", summary.p50_ms);
    println!("p95 ms          : {:.3}", summary.p95_ms);
    println!("p99 ms          : {:.3}", summary.p99_ms);
    println!("max ms          : {:.3}", summary.max_ms);
    println!("checksum        : {}", summary.checksum);
}
