// src/bin/generate_bench_data.rs
//
// CLI binary: writes bench_data/ to disk for sysbench replay.
//
// Usage:
//   cargo run --release --bin generate_bench_data -- [FLAGS]
//
// Flags (all optional, defaults match BenchmarkConfig::default()):
//   --users               <N>   Number of User entities         [1000]
//   --documents           <N>   Number of Document entities     [10000]
//   --policies            <N>   Number of Cedar policies        [100]
//   --requests            <N>   Number of requests in .jsonl    [100000]
//   --departments         <N>   Distinct departments            [8]
//   --teams               <N>   Teams per department            [4]
//   --attributes-per-entity <N> Extra attrs per entity          [3]
//   --seed                <N>   LCG seed                        [42]
//   --out                 <DIR> Output directory                [bench_data]

use cedar_agent::bench_dataset::{generate_dataset, BenchmarkConfig};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "generate_bench_data", about = "Generate cedar-agent sysbench dataset")]
struct Cli {
    #[arg(long, default_value_t = 1_000)]
    users: usize,

    #[arg(long, default_value_t = 10_000)]
    documents: usize,

    #[arg(long, default_value_t = 100)]
    policies: usize,

    #[arg(long, default_value_t = 100_000)]
    requests: usize,

    #[arg(long, default_value_t = 8)]
    departments: usize,

    #[arg(long, default_value_t = 4)]
    teams: usize,

    #[arg(long, default_value_t = 3)]
    attributes_per_entity: usize,

    #[arg(long, default_value_t = 42)]
    seed: u64,

    #[arg(long, default_value = "bench_data")]
    out: String,
}

fn main() {
    let cli = Cli::parse();

    let cfg = BenchmarkConfig {
        users:                 cli.users,
        documents:             cli.documents,
        policies:              cli.policies,
        requests:              cli.requests,
        departments:           cli.departments,
        teams:                 cli.teams,
        attributes_per_entity: cli.attributes_per_entity,
        allow_ratio:           0.70,
        read_ratio:            0.70,
        update_ratio:          0.20,
        delete_ratio:          0.10,
        random_seed:           cli.seed,
    };

    println!("Generating dataset:\n{cfg:#?}\n");
    generate_dataset(&cfg, &cli.out);

    println!("\nNext steps:");
    println!("  1. Load entities into cedar-agent:");
    println!("       curl -X PUT -H 'Content-Type: application/json' \\");
    println!("            -d @{}/entities.json \\", cli.out);
    println!("            http://localhost:8180/v1/data");
    println!();
    println!("  2. Load policies into cedar-agent:");
    println!("       curl -X PUT -H 'Content-Type: application/json' \\");
    println!("            -d @{}/policies.json \\", cli.out);
    println!("            http://localhost:8180/v1/policies");
    println!();
    println!("  3. Run sysbench:");
    println!("       sysbench benchmark/cedar.lua \\");
    println!("         --requests-file={}/requests.jsonl \\", cli.out);
    println!("         --cedar-url=http://127.0.0.1:8180 \\");
    println!("         --threads=32 --time=60 run");
}