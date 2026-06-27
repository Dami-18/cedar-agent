// src/bench_dataset.rs
//
// Reproducible workload generator for Cedar / PolTree benchmarks

use cedar_policy::{
Context, Entities, Entity, EntityId, EntityTypeName, EntityUid,
    PolicySet, Request,
};
use std::collections::HashMap;
use std::str::FromStr;

/// Parameters that fully describe one benchmark workload
#[derive(Debug, Clone)]
pub struct DatasetConfig {
    /// Number of User entities.
    pub num_users: usize,
    /// Number of Document entities.
    pub num_docs: usize,
    /// Number of permit policies to generate.
    pub num_policies: usize,
    /// Number of scalar attributes added to each User entity.
    pub attrs_per_user: usize,
    /// Number of scalar attributes added to each Document entity.
    pub attrs_per_doc: usize,
    /// How many distinct values each attribute can take.
    /// Lower → more entities share the same value → tree is shallower.
    pub values_per_attr: usize,
    /// Seed for the deterministic pseudo-random decisions.
    pub seed: u64,
}

impl Default for DatasetConfig {
    fn default() -> Self {
        Self {
            num_users: 100,
            num_docs: 100,
            num_policies: 50,
            attrs_per_user: 3,
            attrs_per_doc: 3,
            values_per_attr: 10,
            seed: 42,
        }
    }
}

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x_dead_beef_cafe_babe)
    }
    fn next(&mut self) -> u64 {
        // Knuth's multiplicative LCG (64-bit)
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next() as usize % (hi - lo))
    }
}

pub struct BenchDataset {
    pub policy_set: PolicySet,
    pub entities: Entities,
    /// A single representative `Request` used as the hot-path query.
    pub sample_request: Request,
    /// All generated requests (for variety benchmarks).
    pub all_requests: Vec<Request>,
}

pub fn generate(cfg: &DatasetConfig) -> BenchDataset {
    let mut rng = Lcg::new(cfg.seed);

    let user_type: EntityTypeName = "User".parse().unwrap();
    let doc_type: EntityTypeName = "Document".parse().unwrap();
    let action_type: EntityTypeName = "Action".parse().unwrap();

    // We use simple strings: "dept_0", "dept_1", …
    // The attribute names themselves are "ua0", "ua1", … (user) and "da0", "da1", … (doc).
    let user_attr_names: Vec<String> = (0..cfg.attrs_per_user).map(|i| format!("ua{i}")).collect();
    let doc_attr_names: Vec<String> = (0..cfg.attrs_per_doc).map(|i| format!("da{i}")).collect();

    let values_for = |prefix: &str, attr_idx: usize, val_idx: usize| -> String {
        format!("{prefix}_{attr_idx}_{val_idx}")
    };

    let mut entity_list: Vec<Entity> = Vec::new();

    // Action entity (single shared action)
    let action_uid = EntityUid::from_type_name_and_id(
        action_type.clone(),
        EntityId::from_str("view").unwrap(),
    );
    entity_list.push(
        Entity::new(action_uid.clone(), HashMap::new(), std::collections::HashSet::new()).unwrap(),
    );

    // User entities
    let mut user_uids: Vec<EntityUid> = Vec::new();
    for i in 0..cfg.num_users {
        let uid = EntityUid::from_type_name_and_id(
            user_type.clone(),
            EntityId::from_str(&format!("user_{i}")).unwrap(),
        );
        let mut attrs: HashMap<String, cedar_policy::RestrictedExpression> = HashMap::new();
        for (ai, aname) in user_attr_names.iter().enumerate() {
            let val_idx = rng.range(0, cfg.values_per_attr);
            let val_str = values_for("uv", ai, val_idx);
            attrs.insert(
                aname.clone(),
                cedar_policy::RestrictedExpression::new_string(val_str),
            );
        }
        entity_list.push(
            Entity::new(uid.clone(), attrs, std::collections::HashSet::new()).unwrap(),
        );
        user_uids.push(uid);
    }

    // Document entities
    let mut doc_uids: Vec<EntityUid> = Vec::new();
    for i in 0..cfg.num_docs {
        let uid = EntityUid::from_type_name_and_id(
            doc_type.clone(),
            EntityId::from_str(&format!("doc_{i}")).unwrap(),
        );
        let mut attrs: HashMap<String, cedar_policy::RestrictedExpression> = HashMap::new();
        for (ai, aname) in doc_attr_names.iter().enumerate() {
            let val_idx = rng.range(0, cfg.values_per_attr);
            let val_str = values_for("dv", ai, val_idx);
            attrs.insert(
                aname.clone(),
                cedar_policy::RestrictedExpression::new_string(val_str),
            );
        }
        entity_list.push(
            Entity::new(uid.clone(), attrs, std::collections::HashSet::new()).unwrap(),
        );
        doc_uids.push(uid);
    }

    let entities = Entities::from_entities(entity_list, None).unwrap();

    // Each policy constrains one user-attribute value AND one doc-attribute value.
    // Template:
    //   permit(
    //     principal == User::"user_<U>",
    //     action    == Action::"view",
    //     resource  == Document::"doc_<D>"
    //   ) when { principal.ua<X> == "<val>" && resource.da<Y> == "<val>" };
    //
    // We also generate a fraction of broader "department-level" policies that
    // only constrain attributes (no specific UID scope) to give PolTree's
    // tree an interesting structure.

    let mut policy_src = String::new();

    for p in 0..cfg.num_policies {
        let user_idx = rng.range(0, cfg.num_users);
        let doc_idx = rng.range(0, cfg.num_docs);

        // Pick a random user-attr constraint
        let ua_idx = if cfg.attrs_per_user > 0 {
            rng.range(0, cfg.attrs_per_user)
        } else {
            0
        };
        let ua_val_idx = rng.range(0, cfg.values_per_attr);
        let ua_val = values_for("uv", ua_idx, ua_val_idx);
        let ua_name = &user_attr_names[ua_idx.min(user_attr_names.len().saturating_sub(1))];

        // Pick a random doc-attr constraint
        let da_idx = if cfg.attrs_per_doc > 0 {
            rng.range(0, cfg.attrs_per_doc)
        } else {
            0
        };
        let da_val_idx = rng.range(0, cfg.values_per_attr);
        let da_val = values_for("dv", da_idx, da_val_idx);
        let da_name = &doc_attr_names[da_idx.min(doc_attr_names.len().saturating_sub(1))];

        // Alternate between scoped (specific UID) and attribute-only policies
        let policy = if p % 3 == 0 {
            // Attribute-only: no UID scope, just attr constraints
            format!(
                r#"@id("policy_{p}")
permit(
  principal,
  action == Action::"view",
  resource
) when {{
  principal.{ua_name} == "{ua_val}" &&
  resource.{da_name} == "{da_val}"
}};
"#
            )
        } else {
            // Scoped: specific principal + resource UIDs
            format!(
                r#"@id("policy_{p}")
permit(
  principal == User::"user_{user_idx}",
  action == Action::"view",
  resource == Document::"doc_{doc_idx}"
) when {{
  principal.{ua_name} == "{ua_val}" &&
  resource.{da_name} == "{da_val}"
}};
"#
            )
        };

        policy_src.push_str(&policy);
    }

    let policy_set: PolicySet = policy_src.parse().expect("generated policy text is invalid");

    let mut rng2 = Lcg::new(cfg.seed.wrapping_add(1));
    let mut all_requests: Vec<Request> = Vec::new();

    let request_count = (cfg.num_users * cfg.num_docs).min(200).max(10);
    for _ in 0..request_count {
        let u_idx = rng2.range(0, user_uids.len().max(1));
        let d_idx = rng2.range(0, doc_uids.len().max(1));
        let req = Request::new(
            user_uids[u_idx].clone(),
            action_uid.clone(),
            doc_uids[d_idx].clone(),
            Context::empty(),
            None,
        )
        .unwrap();
        all_requests.push(req);
    }

    let sample_request = {
        let u_idx = rng2.range(0, user_uids.len().max(1));
        let d_idx = rng2.range(0, doc_uids.len().max(1));
        Request::new(
            user_uids[u_idx].clone(),
            action_uid.clone(),
            doc_uids[d_idx].clone(),
            Context::empty(),
            None,
        )
        .unwrap()
    };

    BenchDataset {
        policy_set,
        entities,
        sample_request,
        all_requests,
    }
}

/// N sweeps from `start` to `end` in multiplicative steps.
pub fn policy_scaling_configs(
    start: usize,
    end: usize,
    steps: usize,
    base_users: usize,
    base_docs: usize,
    attrs: usize,
) -> Vec<(usize, DatasetConfig)> {
    log_steps(start, end, steps)
        .into_iter()
        .map(|n| {
            (
                n,
                DatasetConfig {
                    num_users: base_users,
                    num_docs: base_docs,
                    num_policies: n,
                    attrs_per_user: attrs,
                    attrs_per_doc: attrs,
                    values_per_attr: 10,
                    seed: 42,
                },
            )
        })
        .collect()
}

pub fn entity_scaling_configs(
    start: usize,
    end: usize,
    steps: usize,
    base_policies: usize,
    attrs: usize,
) -> Vec<(usize, DatasetConfig)> {
    log_steps(start, end, steps)
        .into_iter()
        .map(|n| {
            (
                n,
                DatasetConfig {
                    num_users: n / 2,
                    num_docs: n / 2,
                    num_policies: base_policies,
                    attrs_per_user: attrs,
                    attrs_per_doc: attrs,
                    values_per_attr: 10,
                    seed: 42,
                },
            )
        })
        .collect()
}

pub fn attr_scaling_configs(
    start: usize,
    end: usize,
    steps: usize,
    base_users: usize,
    base_docs: usize,
    base_policies: usize,
) -> Vec<(usize, DatasetConfig)> {
    (start..=end)
        .step_by(((end - start) / steps.max(1)).max(1))
        .map(|a| {
            (
                a,
                DatasetConfig {
                    num_users: base_users,
                    num_docs: base_docs,
                    num_policies: base_policies,
                    attrs_per_user: a,
                    attrs_per_doc: a,
                    values_per_attr: 10,
                    seed: 42,
                },
            )
        })
        .collect()
}

fn log_steps(start: usize, end: usize, steps: usize) -> Vec<usize> {
    if steps <= 1 {
        return vec![start, end];
    }
    let ratio = (end as f64 / start as f64).powf(1.0 / (steps - 1) as f64);
    (0..steps)
        .map(|i| (start as f64 * ratio.powi(i as i32)).round() as usize)
        .collect()
}