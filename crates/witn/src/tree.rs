//! Turns the flat process list into a parent→child forest and flattens it back
//! in display order with a `depth` on each node, so the CLI and GUI can indent
//! children under the project root that spawned them.

use crate::model::NodeProc;
use std::collections::HashMap;

/// Nests processes: a node whose parent PID is another node in the set becomes
/// its child; everything else is a root. Returns the forest flattened in a
/// stable DFS order with `depth` filled in.
///
/// A parent is only accepted if it started no later than the child — this
/// rejects a false parent produced by PID recycling (a fresh process reusing a
/// dead one's PID).
pub fn build(procs: Vec<NodeProc>) -> Vec<NodeProc> {
    let start_of: HashMap<u32, u64> = procs.iter().map(|p| (p.pid, p.start_filetime)).collect();

    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        let parent_is_node = p.ppid != p.pid
            && start_of
                .get(&p.ppid)
                .is_some_and(|&parent_start| parent_start <= p.start_filetime);
        if parent_is_node {
            children.entry(p.ppid).or_default().push(i);
        } else {
            roots.push(i);
        }
    }

    // Stable, human order: by app name (case-insensitive), then PID.
    let sort_key = |i: &usize| {
        let p = &procs[*i];
        (p.app_name.to_lowercase(), p.pid)
    };
    roots.sort_by_key(sort_key);
    for kids in children.values_mut() {
        kids.sort_by_key(sort_key);
    }

    let mut plan: Vec<(usize, usize)> = Vec::with_capacity(procs.len());
    for &root in &roots {
        emit(root, 0, &procs, &children, &mut plan);
    }

    // Materialize in planned order, moving each proc out of its slot once.
    let mut slots: Vec<Option<NodeProc>> = procs.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(plan.len());
    for (idx, depth) in plan {
        if let Some(mut p) = slots[idx].take() {
            p.depth = depth;
            out.push(p);
        }
    }
    out
}

/// Depth-first pre-order: emit a node, then each of its (already sorted)
/// children one level deeper.
fn emit(
    idx: usize,
    depth: usize,
    procs: &[NodeProc],
    children: &HashMap<u32, Vec<usize>>,
    plan: &mut Vec<(usize, usize)>,
) {
    plan.push((idx, depth));
    if let Some(kids) = children.get(&procs[idx].pid) {
        for &kid in kids {
            emit(kid, depth + 1, procs, children, plan);
        }
    }
}
