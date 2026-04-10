/// SR formula tree: generation, evaluation, crossover, mutation, formatting.

use std::collections::HashMap;
use std::sync::Arc;

use rand::seq::SliceRandom;
use rand::Rng;

use crate::engine::indicators::IndicatorOutput;
use crate::models::sr_result::{BinaryOpType, PoolLeaf, SrNode, UnaryOpType};

/// Shared pre-computed indicator data. Key = `IndicatorConfig::cache_key_hash()`.
/// Wrapped in `Arc` so all rayon threads share one allocation (read-only after construction).
pub type SrCache = Arc<HashMap<u64, Arc<IndicatorOutput>>>;

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Evaluate a node at bar index `idx`. Returns 0.0 on cache miss (shouldn't happen
/// with a correctly pre-computed cache) and uses protected arithmetic throughout.
pub fn evaluate(node: &SrNode, idx: usize, cache: &SrCache) -> f64 {
    match node {
        SrNode::Constant(v) => *v,
        SrNode::IndicatorLeaf { config, buffer_index } => {
            let key = config.cache_key_hash();
            match cache.get(&key) {
                None => 0.0,
                Some(output) => {
                    let vec: Option<&Vec<f64>> = match buffer_index {
                        0 => Some(&output.primary),
                        1 => output.secondary.as_ref(),
                        2 => output.tertiary.as_ref(),
                        _ => None,
                    };
                    vec.and_then(|v| v.get(idx).copied()).unwrap_or(0.0)
                }
            }
        }
        SrNode::BinaryOp { op, left, right } => {
            let l = evaluate(left, idx, cache);
            let r = evaluate(right, idx, cache);
            match op {
                BinaryOpType::Add => l + r,
                BinaryOpType::Sub => l - r,
                BinaryOpType::Mul => l * r,
                BinaryOpType::ProtectedDiv => {
                    if r.abs() < 1e-10 { 0.0 } else { l / r }
                }
            }
        }
        SrNode::UnaryOp { op, child } => {
            let v = evaluate(child, idx, cache);
            match op {
                UnaryOpType::Sqrt => v.abs().sqrt(),
                UnaryOpType::Abs => v.abs(),
                UnaryOpType::Log => (v.abs() + 1e-10).ln(),
                UnaryOpType::Neg => -v,
            }
        }
    }
}

// ── Tree Introspection ────────────────────────────────────────────────────────

/// Count total nodes in the tree.
pub fn count_nodes(node: &SrNode) -> usize {
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => 1,
        SrNode::BinaryOp { left, right, .. } => {
            1 + count_nodes(left) + count_nodes(right)
        }
        SrNode::UnaryOp { child, .. } => 1 + count_nodes(child),
    }
}

/// Return the maximum depth of the tree (leaf = depth 1).
pub fn tree_depth(node: &SrNode) -> usize {
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => 1,
        SrNode::BinaryOp { left, right, .. } => 1 + tree_depth(left).max(tree_depth(right)),
        SrNode::UnaryOp { child, .. } => 1 + tree_depth(child),
    }
}

/// Extract all constant values in in-order traversal order.
pub fn extract_constants(node: &SrNode) -> Vec<f64> {
    let mut out = Vec::new();
    extract_inner(node, &mut out);
    out
}

fn extract_inner(node: &SrNode, out: &mut Vec<f64>) {
    match node {
        SrNode::Constant(v) => out.push(*v),
        SrNode::IndicatorLeaf { .. } => {}
        SrNode::BinaryOp { left, right, .. } => {
            extract_inner(left, out);
            extract_inner(right, out);
        }
        SrNode::UnaryOp { child, .. } => extract_inner(child, out),
    }
}

/// Replace all constant values with entries from `values` (in-order traversal).
pub fn replace_constants(node: &mut SrNode, values: &[f64], idx: &mut usize) {
    match node {
        SrNode::Constant(v) => {
            if *idx < values.len() {
                *v = values[*idx];
                *idx += 1;
            }
        }
        SrNode::IndicatorLeaf { .. } => {}
        SrNode::BinaryOp { left, right, .. } => {
            replace_constants(left, values, idx);
            replace_constants(right, values, idx);
        }
        SrNode::UnaryOp { child, .. } => replace_constants(child, values, idx),
    }
}

// ── Algebraic Simplification ─────────────────────────────────────────────────

/// Simplify a tree by applying algebraic identities and constant folding.
/// Reduces bloat from redundant patterns that accumulate during evolution.
pub fn simplify(node: &SrNode) -> SrNode {
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => node.clone(),
        SrNode::UnaryOp { op, child } => {
            let sc = simplify(child);
            match op {
                // neg(neg(x)) → x
                UnaryOpType::Neg => {
                    if let SrNode::UnaryOp { op: UnaryOpType::Neg, child: inner } = &sc {
                        return (**inner).clone();
                    }
                }
                // abs(abs(x)) → abs(x)
                UnaryOpType::Abs => {
                    if let SrNode::UnaryOp { op: UnaryOpType::Abs, .. } = &sc {
                        return sc;
                    }
                }
                _ => {}
            }
            // Constant folding for unary ops
            if let SrNode::Constant(v) = &sc {
                let result = match op {
                    UnaryOpType::Sqrt => v.abs().sqrt(),
                    UnaryOpType::Abs => v.abs(),
                    UnaryOpType::Log => (v.abs() + 1e-10).ln(),
                    UnaryOpType::Neg => -*v,
                };
                if result.is_finite() {
                    return SrNode::Constant(result);
                }
            }
            SrNode::UnaryOp { op: *op, child: Box::new(sc) }
        }
        SrNode::BinaryOp { op, left, right } => {
            let sl = simplify(left);
            let sr = simplify(right);

            // Constant folding: both children are constants
            if let (SrNode::Constant(l), SrNode::Constant(r)) = (&sl, &sr) {
                let result = match op {
                    BinaryOpType::Add => l + r,
                    BinaryOpType::Sub => l - r,
                    BinaryOpType::Mul => l * r,
                    BinaryOpType::ProtectedDiv => {
                        if r.abs() < 1e-10 { 0.0 } else { l / r }
                    }
                };
                if result.is_finite() {
                    return SrNode::Constant(result);
                }
            }

            match op {
                BinaryOpType::Add => {
                    // x + 0 → x, 0 + x → x
                    if is_zero(&sr) { return sl; }
                    if is_zero(&sl) { return sr; }
                }
                BinaryOpType::Sub => {
                    // x - 0 → x
                    if is_zero(&sr) { return sl; }
                    // x - x → 0 (same indicator leaf)
                    if structurally_equal(&sl, &sr) { return SrNode::Constant(0.0); }
                }
                BinaryOpType::Mul => {
                    // x * 0 → 0, 0 * x → 0
                    if is_zero(&sl) || is_zero(&sr) { return SrNode::Constant(0.0); }
                    // x * 1 → x, 1 * x → x
                    if is_one(&sr) { return sl; }
                    if is_one(&sl) { return sr; }
                }
                BinaryOpType::ProtectedDiv => {
                    // 0 / x → 0
                    if is_zero(&sl) { return SrNode::Constant(0.0); }
                    // x / 1 → x
                    if is_one(&sr) { return sl; }
                    // x / x → 1 (same indicator leaf)
                    if structurally_equal(&sl, &sr) { return SrNode::Constant(1.0); }
                }
            }

            SrNode::BinaryOp { op: *op, left: Box::new(sl), right: Box::new(sr) }
        }
    }
}

fn is_zero(node: &SrNode) -> bool {
    matches!(node, SrNode::Constant(v) if v.abs() < 1e-10)
}

fn is_one(node: &SrNode) -> bool {
    matches!(node, SrNode::Constant(v) if (v - 1.0).abs() < 1e-10)
}

/// Check if two nodes are structurally identical (same tree shape and values).
fn structurally_equal(a: &SrNode, b: &SrNode) -> bool {
    match (a, b) {
        (SrNode::Constant(va), SrNode::Constant(vb)) => (va - vb).abs() < 1e-10,
        (
            SrNode::IndicatorLeaf { config: ca, buffer_index: ba },
            SrNode::IndicatorLeaf { config: cb, buffer_index: bb },
        ) => ba == bb && ca.cache_key_hash() == cb.cache_key_hash(),
        (
            SrNode::BinaryOp { op: oa, left: la, right: ra },
            SrNode::BinaryOp { op: ob, left: lb, right: rb },
        ) => oa == ob && structurally_equal(la, lb) && structurally_equal(ra, rb),
        (
            SrNode::UnaryOp { op: oa, child: ca },
            SrNode::UnaryOp { op: ob, child: cb },
        ) => oa == ob && structurally_equal(ca, cb),
        _ => false,
    }
}

// ── Random Tree Generation ────────────────────────────────────────────────────

/// Generate a random tree using ramped-half-and-half sampling.
///
/// * At `current_depth >= max_depth` → always a leaf.
/// * Probability of choosing a leaf increases linearly with depth.
/// * `const_min_exp`/`const_max_exp`: log₁₀ bounds for constant magnitudes.
pub fn generate_random<R: Rng>(
    current_depth: usize,
    max_depth: usize,
    pool: &[PoolLeaf],
    const_min_exp: f64,
    const_max_exp: f64,
    rng: &mut R,
) -> SrNode {
    let leaf_prob = if current_depth >= max_depth {
        1.0_f64
    } else {
        (current_depth as f64 / max_depth as f64) * 0.65
    };

    if rng.gen::<f64>() < leaf_prob || pool.is_empty() {
        return random_leaf(pool, const_min_exp, const_max_exp, rng);
    }

    // Uniform distribution across all 8 operators — previously Mul had 2/9 probability
    // which biased evolved formulas toward multiplicative structures.
    match rng.gen_range(0_u8..8) {
        0 => SrNode::BinaryOp {
            op: BinaryOpType::Add,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        1 => SrNode::BinaryOp {
            op: BinaryOpType::Sub,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        2 => SrNode::BinaryOp {
            op: BinaryOpType::Mul,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        3 => SrNode::BinaryOp {
            op: BinaryOpType::ProtectedDiv,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        4 => SrNode::UnaryOp {
            op: UnaryOpType::Sqrt,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        5 => SrNode::UnaryOp {
            op: UnaryOpType::Abs,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        6 => SrNode::UnaryOp {
            op: UnaryOpType::Log,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
        _ => SrNode::UnaryOp {
            op: UnaryOpType::Neg,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, const_min_exp, const_max_exp, rng)),
        },
    }
}

fn random_leaf<R: Rng>(pool: &[PoolLeaf], const_min_exp: f64, const_max_exp: f64, rng: &mut R) -> SrNode {
    // 30% constant, 70% indicator leaf (when pool is non-empty).
    if rng.gen::<f64>() < 0.30 || pool.is_empty() {
        // Exponentially distributed magnitude with random sign.
        // Bounds are configurable so constants match the indicator scale of the chosen pool.
        let magnitude = 10_f64.powf(rng.gen_range(const_min_exp..const_max_exp));
        let sign: f64 = if rng.gen::<bool>() { 1.0 } else { -1.0 };
        SrNode::Constant(sign * magnitude)
    } else {
        let leaf = pool.choose(rng).unwrap();
        SrNode::IndicatorLeaf {
            config: leaf.config.clone(),
            buffer_index: leaf.buffer_index,
        }
    }
}

// ── Genetic Operators ─────────────────────────────────────────────────────────

/// Subtree crossover: picks a random subtree from `b` and grafts it onto a random
/// position in `a`. Returns the resulting child. If the result exceeds `max_depth`,
/// the original parent `a` is returned unchanged to prevent bloat.
pub fn subtree_crossover<R: Rng>(a: &SrNode, b: &SrNode, max_depth: usize, rng: &mut R) -> SrNode {
    let na = count_nodes(a);
    let nb = count_nodes(b);
    if na == 0 || nb == 0 {
        return a.clone();
    }
    let donor_idx = rng.gen_range(0..nb);
    let donor = extract_at(b, donor_idx).unwrap_or_else(|| b.clone());
    let target_idx = rng.gen_range(0..na);
    let result = replace_at(a, target_idx, &donor);
    if tree_depth(&result) > max_depth {
        a.clone()
    } else {
        result
    }
}

/// Mutation: replace a random subtree with a freshly generated one.
pub fn mutate<R: Rng>(
    node: &SrNode,
    max_depth: usize,
    pool: &[PoolLeaf],
    const_min_exp: f64,
    const_max_exp: f64,
    rng: &mut R,
) -> SrNode {
    let n = count_nodes(node);
    if n == 0 {
        return generate_random(0, max_depth, pool, const_min_exp, const_max_exp, rng);
    }
    let target = rng.gen_range(0..n);
    // New subtree at half depth to keep size manageable.
    let new_sub = generate_random(0, (max_depth / 2).max(1), pool, const_min_exp, const_max_exp, rng);
    replace_at(node, target, &new_sub)
}

/// Hoist mutation: replace the tree with one of its own subtrees, reducing complexity.
/// Picks a random non-root subtree and returns it as the new tree.
pub fn hoist_mutation<R: Rng>(node: &SrNode, rng: &mut R) -> SrNode {
    let n = count_nodes(node);
    if n <= 1 {
        return node.clone();
    }
    // Pick a random non-root node (index 1..n)
    let target = rng.gen_range(1..n);
    extract_at(node, target).unwrap_or_else(|| node.clone())
}

/// Point mutation: change the operator of a random internal node without altering structure.
/// Binary ops swap to another binary op; unary ops swap to another unary op.
pub fn point_mutation<R: Rng>(node: &SrNode, rng: &mut R) -> SrNode {
    // Collect indices of internal nodes
    let mut internals = Vec::new();
    collect_internal_indices(node, &mut 0, &mut internals);
    if internals.is_empty() {
        return node.clone();
    }
    let target = *internals.choose(rng).unwrap();
    point_mutate_at(node, target, rng)
}

fn collect_internal_indices(node: &SrNode, counter: &mut usize, result: &mut Vec<usize>) {
    let idx = *counter;
    *counter += 1;
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => {}
        SrNode::BinaryOp { left, right, .. } => {
            result.push(idx);
            collect_internal_indices(left, counter, result);
            collect_internal_indices(right, counter, result);
        }
        SrNode::UnaryOp { child, .. } => {
            result.push(idx);
            collect_internal_indices(child, counter, result);
        }
    }
}

fn point_mutate_at<R: Rng>(node: &SrNode, mut idx: usize, rng: &mut R) -> SrNode {
    point_mutate_rec(node, &mut idx, rng).0
}

fn point_mutate_rec<R: Rng>(node: &SrNode, counter: &mut usize, rng: &mut R) -> (SrNode, bool) {
    if *counter == 0 {
        let mutated = match node {
            SrNode::BinaryOp { op, left, right } => {
                let new_op = loop {
                    let candidate = match rng.gen_range(0u8..4) {
                        0 => BinaryOpType::Add,
                        1 => BinaryOpType::Sub,
                        2 => BinaryOpType::Mul,
                        _ => BinaryOpType::ProtectedDiv,
                    };
                    if candidate != *op { break candidate; }
                };
                SrNode::BinaryOp { op: new_op, left: left.clone(), right: right.clone() }
            }
            SrNode::UnaryOp { op, child } => {
                let new_op = loop {
                    let candidate = match rng.gen_range(0u8..4) {
                        0 => UnaryOpType::Sqrt,
                        1 => UnaryOpType::Abs,
                        2 => UnaryOpType::Log,
                        _ => UnaryOpType::Neg,
                    };
                    if candidate != *op { break candidate; }
                };
                SrNode::UnaryOp { op: new_op, child: child.clone() }
            }
            _ => node.clone(),
        };
        return (mutated, true);
    }
    *counter -= 1;
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => (node.clone(), false),
        SrNode::BinaryOp { op, left, right } => {
            let (nl, done) = point_mutate_rec(left, counter, rng);
            if done {
                return (SrNode::BinaryOp { op: *op, left: Box::new(nl), right: right.clone() }, true);
            }
            let (nr, done2) = point_mutate_rec(right, counter, rng);
            (SrNode::BinaryOp { op: *op, left: Box::new(nl), right: Box::new(nr) }, done2)
        }
        SrNode::UnaryOp { op, child } => {
            let (nc, done) = point_mutate_rec(child, counter, rng);
            (SrNode::UnaryOp { op: *op, child: Box::new(nc) }, done)
        }
    }
}

/// Constant perturbation: nudge a random constant in the tree by ±10–30%.
pub fn constant_perturbation<R: Rng>(node: &SrNode, rng: &mut R) -> SrNode {
    let consts = extract_constants(node);
    if consts.is_empty() {
        return node.clone();
    }
    // Pick a random constant by pre-order index
    let mut const_indices = Vec::new();
    collect_constant_indices(node, &mut 0, &mut const_indices);
    if const_indices.is_empty() {
        return node.clone();
    }
    let target = *const_indices.choose(rng).unwrap();
    if let Some(SrNode::Constant(v)) = extract_at(node, target) {
        let factor = 1.0 + rng.gen_range(-0.3_f64..0.3);
        let new_val = v * factor;
        // Keep it finite
        if new_val.is_finite() {
            return replace_at(node, target, &SrNode::Constant(new_val));
        }
    }
    node.clone()
}

fn collect_constant_indices(node: &SrNode, counter: &mut usize, result: &mut Vec<usize>) {
    let idx = *counter;
    *counter += 1;
    match node {
        SrNode::Constant(_) => result.push(idx),
        SrNode::IndicatorLeaf { .. } => {}
        SrNode::BinaryOp { left, right, .. } => {
            collect_constant_indices(left, counter, result);
            collect_constant_indices(right, counter, result);
        }
        SrNode::UnaryOp { child, .. } => {
            collect_constant_indices(child, counter, result);
        }
    }
}

/// Parameter mutation: nudge the period of a randomly chosen `IndicatorLeaf` by one
/// step within the range implied by the pool.
///
/// Only leaves whose `indicator_type` has at least 2 distinct period values in the
/// expanded pool are eligible (i.e. they came from a period-range template entry).
/// Returns a new tree with the period changed, or the original tree if no eligible
/// leaf was found or the step would produce no change.
pub fn mutate_leaf_params<R: Rng>(node: &SrNode, pool: &[PoolLeaf], rng: &mut R) -> SrNode {
    let mut eligible: Vec<usize> = Vec::new();
    let mut counter = 0usize;
    collect_period_leaf_indices(node, pool, &mut counter, &mut eligible);
    if eligible.is_empty() {
        return node.clone();
    }
    let target_idx = *eligible.choose(rng).unwrap();
    if let Some(SrNode::IndicatorLeaf { config, buffer_index }) = extract_at(node, target_idx) {
        // Collect and sort all periods available for this indicator in the expanded pool.
        let mut periods: Vec<usize> = pool.iter()
            .filter(|pl| {
                pl.config.indicator_type == config.indicator_type
                    && pl.buffer_index == buffer_index
                    && pl.config.params.period.is_some()
            })
            .filter_map(|pl| pl.config.params.period)
            .collect();
        periods.sort_unstable();
        periods.dedup();
        if periods.len() < 2 {
            return node.clone();
        }
        let cur_period = config.params.period.unwrap_or(periods[0]);
        let cur_pos = periods.iter().position(|&p| p == cur_period).unwrap_or(0);
        let new_pos = if rng.gen::<bool>() {
            (cur_pos + 1).min(periods.len() - 1)
        } else {
            cur_pos.saturating_sub(1)
        };
        let new_period = periods[new_pos];
        if new_period != cur_period {
            let mut new_config = config.clone();
            new_config.params.period = Some(new_period);
            let new_leaf = SrNode::IndicatorLeaf { config: new_config, buffer_index };
            return replace_at(node, target_idx, &new_leaf);
        }
    }
    node.clone()
}

/// Collect pre-order indices of `IndicatorLeaf` nodes that have >= 2 distinct period
/// values available for their indicator type in the expanded pool.
fn collect_period_leaf_indices(
    node: &SrNode,
    pool: &[PoolLeaf],
    counter: &mut usize,
    result: &mut Vec<usize>,
) {
    let current_idx = *counter;
    *counter += 1;
    match node {
        SrNode::IndicatorLeaf { config, buffer_index } => {
            // Use a simple count instead of a HashSet to keep it allocation-light.
            let mut first: Option<usize> = None;
            let mut has_second = false;
            for pl in pool {
                if pl.config.indicator_type == config.indicator_type
                    && pl.buffer_index == *buffer_index
                {
                    if let Some(p) = pl.config.params.period {
                        match first {
                            None => { first = Some(p); }
                            Some(f) if p != f => { has_second = true; break; }
                            _ => {}
                        }
                    }
                }
            }
            if has_second {
                result.push(current_idx);
            }
        }
        SrNode::Constant(_) => {}
        SrNode::BinaryOp { left, right, .. } => {
            collect_period_leaf_indices(left, pool, counter, result);
            collect_period_leaf_indices(right, pool, counter, result);
        }
        SrNode::UnaryOp { child, .. } => {
            collect_period_leaf_indices(child, pool, counter, result);
        }
    }
}

// ── Subtree Access Helpers ────────────────────────────────────────────────────

fn extract_at(node: &SrNode, mut idx: usize) -> Option<SrNode> {
    extract_rec(node, &mut idx)
}

fn extract_rec(node: &SrNode, counter: &mut usize) -> Option<SrNode> {
    if *counter == 0 {
        return Some(node.clone());
    }
    *counter -= 1;
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => None,
        SrNode::BinaryOp { left, right, .. } => {
            extract_rec(left, counter).or_else(|| extract_rec(right, counter))
        }
        SrNode::UnaryOp { child, .. } => extract_rec(child, counter),
    }
}

fn replace_at(node: &SrNode, mut idx: usize, replacement: &SrNode) -> SrNode {
    replace_rec(node, &mut idx, replacement).0
}

fn replace_rec(node: &SrNode, counter: &mut usize, rep: &SrNode) -> (SrNode, bool) {
    if *counter == 0 {
        return (rep.clone(), true);
    }
    *counter -= 1;
    match node {
        SrNode::Constant(_) | SrNode::IndicatorLeaf { .. } => (node.clone(), false),
        SrNode::BinaryOp { op, left, right } => {
            let (nl, done) = replace_rec(left, counter, rep);
            if done {
                return (
                    SrNode::BinaryOp {
                        op: *op,
                        left: Box::new(nl),
                        right: right.clone(),
                    },
                    true,
                );
            }
            let (nr, done2) = replace_rec(right, counter, rep);
            (
                SrNode::BinaryOp {
                    op: *op,
                    left: Box::new(nl),
                    right: Box::new(nr),
                },
                done2,
            )
        }
        SrNode::UnaryOp { op, child } => {
            let (nc, done) = replace_rec(child, counter, rep);
            (SrNode::UnaryOp { op: *op, child: Box::new(nc) }, done)
        }
    }
}

// ── Human-Readable Formatting ─────────────────────────────────────────────────

/// Format a node as a human-readable formula string.
pub fn format_tree(node: &SrNode) -> String {
    match node {
        SrNode::Constant(v) => format!("{:.4}", v),
        SrNode::IndicatorLeaf { config, buffer_index } => {
            let key = config.cache_key();
            if *buffer_index == 0 {
                key
            } else {
                format!("{}[buf{}]", key, buffer_index)
            }
        }
        SrNode::BinaryOp { op, left, right } => {
            let sym = match op {
                BinaryOpType::Add => "+",
                BinaryOpType::Sub => "-",
                BinaryOpType::Mul => "*",
                BinaryOpType::ProtectedDiv => "/",
            };
            format!("({} {} {})", format_tree(left), sym, format_tree(right))
        }
        SrNode::UnaryOp { op, child } => {
            let name = match op {
                UnaryOpType::Sqrt => "sqrt",
                UnaryOpType::Abs => "abs",
                UnaryOpType::Log => "log",
                UnaryOpType::Neg => "neg",
            };
            format!("{}({})", name, format_tree(child))
        }
    }
}
