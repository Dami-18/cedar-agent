//   bench_data/
//     entities.json    – Cedar entity store (users + documents)
//     policies.json   – Realistic ABAC policies
//     requests.jsonl   – Authorization workload (one JSON object per line)
//     metadata.json    – Dataset summary

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkConfig {
    /// Number of User entities
    pub users: usize,
    /// Number of Document entities
    pub documents: usize,
    /// Number of Cedar policies to generate
    pub policies: usize,
    /// Number of authorization requests to write to requests.jsonl
    pub requests: usize,
    /// Number of distinct departments (e.g. engineering, finance)
    pub departments: usize,
    /// Number of distinct teams inside each department
    pub teams: usize,
    /// Extra scalar attributes per entity beyond the structural ones
    pub attributes_per_entity: usize,
    /// Target fraction of Allow decisions (0.0–1.0); actual ratio is reported
    pub allow_ratio: f64,
    /// Fraction of requests using the "read" action
    pub read_ratio: f64,
    /// Fraction of requests using the "update" action
    pub update_ratio: f64,
    /// Fraction of requests using the "delete" action
    pub delete_ratio: f64,
    /// LCG seed
    pub random_seed: u64,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            users: 1_000,
            documents: 10_000,
            policies: 100,
            requests: 100_000,
            departments: 8,
            teams: 4,
            attributes_per_entity: 3,
            allow_ratio: 0.70,
            read_ratio: 0.70,
            update_ratio: 0.20,
            delete_ratio: 0.10,
            random_seed: 42,
        }
    }
}
// lcg seed - linear congruential generator 
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xdead_beef_cafe_babe)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// Uniform integer in [lo, hi)
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo { return lo; }
        // NOT `next() % span`: a linear congruential generator's LOW bits
        // have a much shorter effective period than its high bits (the
        // low k bits of an LCG with modulus 2^64 cycle with period at most
        // 2^k), so `% span` for a power-of-two span reads directly off that
        // short cycle -- e.g. `% 8` was observed to only ever produce 4 of
        // its 8 possible residues here. Instead take the HIGH bits via a
        // widening multiply (the standard unbiased bounded-range technique,
        // as used by e.g. Lemire's method / PCG): multiply the full 64-bit
        // output by the span and keep the top 64 bits of the 128-bit
        // product, which draws on the LCG's high-quality high bits instead.
        let span = (hi - lo) as u64;
        let scaled = ((self.next() as u128 * span as u128) >> 64) as u64;
        lo + scaled as usize
    }

    /// Uniform float in [0, 1)
    fn frac(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Pick an index biased toward the first `hot_count` items.
    /// `hot_weight` = how many times more likely a hot item is vs a cold one.
    fn biased(&mut self, total: usize, hot_count: usize, hot_weight: f64) -> usize {
        let hot = hot_count.min(total);
        let hot_mass  = hot as f64 * hot_weight;
        let cold_mass = (total - hot) as f64;
        let p_hot = hot_mass / (hot_mass + cold_mass);
        if self.frac() < p_hot {
            self.range(0, hot)
        } else {
            self.range(hot, total)
        }
    }
}

fn dept_name(i: usize) -> String {
    const NAMES: &[&str] = &[
        "engineering", "finance", "legal", "hr",
        "sales", "ops", "research", "security",
    ];
    // Beyond the named set, synthesize a distinct name per index rather than
    // collapsing everything onto one shared "dept_other" value -- otherwise
    // `--departments N` for N > 8 would silently produce far fewer than N
    // distinct attribute values, understating attribute-value cardinality.
    match NAMES.get(i) {
        Some(name) => name.to_string(),
        None => format!("dept_{i}"),
    }
}

fn team_name(dept: usize, team: usize) -> String {
    format!("{}_{}", dept_name(dept), team)
}

fn clearance_str(level: usize) -> &'static str {
    match level {
        0 => "public",
        1 => "internal",
        2 => "confidential",
        _ => "secret",
    }
}

fn action_for(roll: f64, cfg: &BenchmarkConfig) -> &'static str {
    if roll < cfg.read_ratio { "read" }
    else if roll < cfg.read_ratio + cfg.update_ratio { "update" }
    else { "delete" }
}

struct UserMeta {
    id:              String,
    dept_idx:        usize,
    team_idx:        usize,
    clearance_level: usize,
    is_manager:      bool,
}

struct DocMeta {
    id:              String,
    owner_idx:       usize,
    dept_idx:        usize,
    clearance_level: usize,
}

fn build_entities(cfg: &BenchmarkConfig, rng: &mut Lcg) -> (Vec<UserMeta>, Vec<DocMeta>, Value) {
    let mut arr: Vec<Value> = Vec::new();

    // Actions
    for action in &["read", "update", "delete", "admin"] {
        arr.push(json!({
            "uid":     { "type": "Action", "id": action },
            "attrs":   {},
            "parents": []
        }));
    }

    // Users
    let manager_every = (cfg.users / (cfg.departments * cfg.teams).max(1)).max(5);
    let mut users: Vec<UserMeta> = Vec::with_capacity(cfg.users);

    for i in 0..cfg.users {
        let dept_idx  = rng.range(0, cfg.departments);
        let team_idx  = rng.range(0, cfg.teams);
        let cl        = rng.range(0, 4);
        let is_mgr    = i % manager_every == 0;

        let mut attrs = json!({
            "department":  dept_name(dept_idx),
            "team":        team_name(dept_idx, team_idx),
            "clearance":   clearance_str(cl),
            "is_manager":  is_mgr,
            "employee_id": format!("emp_{i:06}"),
        });

        for k in 0..cfg.attributes_per_entity {
            attrs[format!("attr_{k}")] = json!(format!("val_{}_{}", k, rng.range(0, 10)));
        }

        let id = format!("user_{i:06}");
        arr.push(json!({
            "uid":     { "type": "User", "id": &id },
            "attrs":   attrs,
            "parents": []
        }));
        users.push(UserMeta { id, dept_idx, team_idx, clearance_level: cl, is_manager: is_mgr });
    }

    // Documents
    let mut docs: Vec<DocMeta> = Vec::with_capacity(cfg.documents);

    for i in 0..cfg.documents {
        let owner_idx = rng.range(0, cfg.users);
        let dept_idx  = users[owner_idx].dept_idx;
        let cl        = rng.range(0, 4);

        let mut attrs = json!({
            "owner":      &users[owner_idx].id,
            "department": dept_name(dept_idx),
            "clearance":  clearance_str(cl),
            "doc_type":   if i % 3 == 0 { "report" } else if i % 3 == 1 { "contract" } else { "memo" },
        });

        for k in 0..cfg.attributes_per_entity {
            attrs[format!("attr_{k}")] = json!(format!("val_{}_{}", k, rng.range(0, 10)));
        }

        let id = format!("doc_{i:07}");
        arr.push(json!({
            "uid":     { "type": "Document", "id": &id },
            "attrs":   attrs,
            "parents": []
        }));
        docs.push(DocMeta { id, owner_idx, dept_idx, clearance_level: cl });
    }

    (users, docs, Value::Array(arr))
}
//
// 7 templates cycling across N policies so a mix is always present.
//
// All of these are restricted to flat `entity.attr == <literal>` equality
// constraints (ANDed together) plus `is`/`==` scope constraints and `permit`
// effect only -- the subset PolTree's N-ary indexing can fully represent (see
// cedar-policy-core::poltree::policy_is_fully_indexed). There is intentionally
// no cross-attribute comparison (e.g. `resource.owner == principal.x`), no
// `in`/hierarchy constraint, and no `forbid` policy here: none of those are
// representable in PolTree's node-splitting model (each split partitions an
// attribute's finite literal value domain, which requires the compared value
// to be a policy-time constant), so generating them would force every request
// through PolTree's real-Cedar-evaluator fallback and understate PolTree's own
// indexed-lookup performance. See /home/damiya/Downloads/Poltree.pdf sec. 2 --
// the published rule model itself is scoped to attribute-vs-literal
// conjunctions, which is what this generator reproduces.
//
//   0  same-department read
//   1  same-department update (public docs only)
//   2  team-level read
//   3  clearance-gated read
//   4  manager admin
//   5  internal-clearance read
//   6  public global read

const N_POLICY_TEMPLATES: usize = 7;

#[derive(Debug, Serialize)]
struct PolicyEntry{
    id: String,
    content: String,
}

fn build_policies(cfg: &BenchmarkConfig) -> Vec<PolicyEntry> {
    let mut policies = Vec::with_capacity(cfg.policies);

    for i in 0..cfg.policies {
        let dept = i % cfg.departments;

        let policy = match i % N_POLICY_TEMPLATES {
            // Department read
            0 => PolicyEntry {
                id: format!("dept-read-{i}"),
                content: format!(
                    r#"permit(principal is User, action == Action::"read", resource is Document) when {{ principal.department == "{}" && resource.department == "{}" }};"#,
                    dept_name(dept),
                    dept_name(dept),
                ),
            },

            // Department update
            1 => PolicyEntry {
                id: format!("dept-update-{i}"),
                content: format!(
                    r#"permit(principal is User, action == Action::"update", resource is Document) when {{ principal.department == "{}" && resource.department == "{}" && resource.clearance == "public" }};"#,
                    dept_name(dept),
                    dept_name(dept),
                ),
            },

            // Team read
            2 => PolicyEntry {
                id: format!("team-read-{i}"),
                content: format!(
                    r#"permit(principal is User, action == Action::"read", resource is Document) when {{ principal.team == "{}" && resource.department == "{}" }};"#,
                    team_name(dept, i % cfg.teams),
                    dept_name(dept),
                ),
            },

            // Clearance read
            3 => {
                let cl = clearance_str(i % 4);
                PolicyEntry {
                    id: format!("clearance-read-{i}"),
                    content: format!(
                        r#"permit(principal is User, action == Action::"read", resource is Document) when {{ principal.clearance == "{cl}" && resource.clearance == "public" }};"#
                    ),
                }
            }

            // Manager admin
            4 => PolicyEntry {
                id: format!("manager-admin-{i}"),
                content: format!(
                    r#"permit(principal is User, action == Action::"admin", resource is Document) when {{ principal.is_manager == true && principal.department == "{}" && resource.department == "{}" }};"#,
                    dept_name(dept),
                    dept_name(dept),
                ),
            },

            // Internal read
            5 => PolicyEntry {
                id: format!("internal-read-{i}"),
                content: format!(
                    r#"permit(principal is User, action == Action::"read", resource is Document) when {{ principal.clearance == "internal" && resource.clearance == "internal" && resource.department == "{}" }};"#,
                    dept_name(dept),
                ),
            },

            // Public read
            6 => PolicyEntry {
                id: format!("public-read-{i}"),
                content: String::from(
                    r#"permit(principal, action == Action::"read", resource is Document) when { resource.clearance == "public" };"#,
                ),
            },

            _ => unreachable!(),
        };

        policies.push(policy);
    }

    policies
}

#[derive(Serialize)]
struct AuthRequest {
    principal: String,
    action:    String,
    resource:  String,
}

fn build_requests(
    cfg: &BenchmarkConfig,
    users: &[UserMeta],
    docs:  &[DocMeta],
    rng:   &mut Lcg,
) -> (Vec<AuthRequest>, f64) {
    // Top 5% of users generate 50% of traffic
    let hot_users = (cfg.users / 20).max(1);
    // Top 2% of docs attract 30% of accesses
    let hot_docs  = (cfg.documents / 50).max(1);

    let mut reqs:    Vec<AuthRequest> = Vec::with_capacity(cfg.requests);
    let mut n_allow: usize = 0;

    for _ in 0..cfg.requests {
        // Principal selection (biased toward hot users)
        let u_idx = rng.biased(cfg.users, hot_users, 5.0);
        let user  = &users[u_idx];

        // 80% same-department access, 20% cross-department
        let d_idx = if rng.frac() < 0.80 {
            // Walk forward from a random hot-biased start until we find a same-dept doc
            let start = rng.biased(cfg.documents, hot_docs, 3.0);
            let mut found = start;
            for off in 0..cfg.documents {
                let c = (start + off) % cfg.documents;
                if docs[c].dept_idx == user.dept_idx {
                    found = c;
                    break;
                }
            }
            found
        } else {
            rng.biased(cfg.documents, hot_docs, 3.0)
        };

        let doc    = &docs[d_idx];
        let action = action_for(rng.frac(), cfg);

        // Heuristic Allow/Deny (approximates what the Cedar policies actually decide).
        // No `is_owner` term here: the owner-based policy template was removed
        // (see `build_policies`) since `resource.owner == principal.employee_id`
        // is a cross-attribute comparison PolTree cannot index.
        let same_dept   = doc.dept_idx == user.dept_idx;
        let cl_ok       = user.clearance_level >= doc.clearance_level;
        let public_doc  = doc.clearance_level == 0;

        let allow = (same_dept && action == "read"   && cl_ok)
            || (same_dept && action == "update"  && public_doc)
            || (user.is_manager && action == "admin" && same_dept)
            || (action == "read" && public_doc);

        if allow { n_allow += 1; }

        reqs.push(AuthRequest {
            principal: format!("User::\"{}\"",     user.id),
            action:    format!("Action::\"{}\"",   action),
            resource:  format!("Document::\"{}\"", doc.id),
        });
    }

    let ratio = n_allow as f64 / cfg.requests.max(1) as f64;
    (reqs, ratio)
}

/// Generate the complete benchmark dataset and write it to `output_dir`.
pub fn generate_dataset(cfg: &BenchmarkConfig, output_dir: &str) {
    let dir = Path::new(output_dir);
    fs::create_dir_all(dir).expect("could not create output directory");

    let mut rng = Lcg::new(cfg.random_seed);

    // Entities
    let (users, docs, entity_json) = build_entities(cfg, &mut rng);
    let p = dir.join("entities.json");
    fs::write(&p, serde_json::to_string_pretty(&entity_json).unwrap())
        .expect("write entities.json");
    println!("  wrote {}", p.display());

    // Policies
    let policies = build_policies(cfg);
    let p = dir.join("policies.json");
    fs::write(&p, serde_json::to_string_pretty(&policies).unwrap())
        .expect("write policies.json");
    println!("  wrote {}", p.display());

    // Requests
    let (requests, actual_allow) = build_requests(cfg, &users, &docs, &mut rng);
    let p = dir.join("requests.jsonl");
    {
        let mut lines = String::new();
        for r in &requests {
            lines.push_str(&serde_json::to_string(r).unwrap());
            lines.push('\n');
        }
        fs::write(&p, lines).expect("write requests.jsonl");
    }
    println!("  wrote {} ({} requests)", p.display(), requests.len());

    // Metadata
    let meta = json!({
        "users":              cfg.users,
        "documents":          cfg.documents,
        "policies":           cfg.policies,
        "requests":           cfg.requests,
        "departments":        cfg.departments,
        "teams":              cfg.teams,
        "attrs_per_entity":   cfg.attributes_per_entity,
        "allow_ratio":        actual_allow,
        "seed":               cfg.random_seed,
        "action_distribution": {
            "read":   cfg.read_ratio,
            "update": cfg.update_ratio,
            "delete": cfg.delete_ratio,
        }
    });
    let p = dir.join("metadata.json");
    fs::write(&p, serde_json::to_string_pretty(&meta).unwrap())
        .expect("write metadata.json");
    println!("  wrote {}", p.display());
}