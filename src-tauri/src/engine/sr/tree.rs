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
                    if r.abs() < 1e-6 { 0.0 } else { l / r }
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

// ── Random Tree Generation ────────────────────────────────────────────────────

/// Generate a random tree using ramped-half-and-half sampling.
///
/// * At `current_depth >= max_depth` → always a leaf.
/// * Probability of choosing a leaf increases linearly with depth.
pub fn generate_random<R: Rng>(
    current_depth: usize,
    max_depth: usize,
    pool: &[PoolLeaf],
    rng: &mut R,
) -> SrNode {
    let leaf_prob = if current_depth >= max_depth {
        1.0_f64
    } else {
        (current_depth as f64 / max_depth as f64) * 0.65
    };

    if rng.gen::<f64>() < leaf_prob || pool.is_empty() {
        return random_leaf(pool, rng);
    }

    match rng.gen_range(0_u8..9) {
        0 => SrNode::BinaryOp {
            op: BinaryOpType::Add,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        1 => SrNode::BinaryOp {
            op: BinaryOpType::Sub,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        2 | 3 => SrNode::BinaryOp {
            op: BinaryOpType::Mul,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        4 => SrNode::BinaryOp {
            op: BinaryOpType::ProtectedDiv,
            left: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
            right: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        5 => SrNode::UnaryOp {
            op: UnaryOpType::Sqrt,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        6 => SrNode::UnaryOp {
            op: UnaryOpType::Abs,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        7 => SrNode::UnaryOp {
            op: UnaryOpType::Log,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
        _ => SrNode::UnaryOp {
            op: UnaryOpType::Neg,
            child: Box::new(generate_random(current_depth + 1, max_depth, pool, rng)),
        },
    }
}

fn random_leaf<R: Rng>(pool: &[PoolLeaf], rng: &mut R) -> SrNode {
    // 30% constant, 70% indicator leaf (when pool is non-empty).
    if rng.gen::<f64>() < 0.30 || pool.is_empty() {
        // Sample from a wide range so constants can act as meaningful offsets
        // for indicator-scale values (e.g. RSI 0-100, CCI ±200, prices).
        // Exponentially distributed magnitude with random sign.
        let magnitude = 10_f64.powf(rng.gen_range(-1.0_f64..2.5)); // 0.1 … ~316
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
/// position in `a`. Returns the resulting child.
pub fn subtree_crossover<R: Rng>(a: &SrNode, b: &SrNode, rng: &mut R) -> SrNode {
    let na = count_nodes(a);
    let nb = count_nodes(b);
    if na == 0 || nb == 0 {
        return a.clone();
    }
    let donor_idx = rng.gen_range(0..nb);
    let donor = extract_at(b, donor_idx).unwrap_or_else(|| b.clone());
    let target_idx = rng.gen_range(0..na);
    replace_at(a, target_idx, &donor)
}

/// Mutation: replace a random subtree with a freshly generated one.
pub fn mutate<R: Rng>(
    node: &SrNode,
    max_depth: usize,
    pool: &[PoolLeaf],
    rng: &mut R,
) -> SrNode {
    let n = count_nodes(node);
    if n == 0 {
        return generate_random(0, max_depth, pool, rng);
    }
    let target = rng.gen_range(0..n);
    // New subtree at half depth to keep size manageable.
    let new_sub = generate_random(0, (max_depth / 2).max(1), pool, rng);
    replace_at(node, target, &new_sub)
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
