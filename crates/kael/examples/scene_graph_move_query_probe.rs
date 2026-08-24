//! Release probe for repeated dynamic movement in a 100,000-node scene.

use kael::{SceneGraph, SceneRect};
use std::hint::black_box;
use std::time::Duration;
use web_time::Instant;

const NODE_COUNT: usize = 100_000;
const COLUMNS: usize = 400;
const MOVE_QUERY_OPERATIONS: usize = 10_000;
const MOVE_QUERY_BUDGET: Duration = Duration::from_secs(2);

fn main() {
    let build_started = Instant::now();
    let mut graph = SceneGraph::new();
    let mut ids = Vec::with_capacity(NODE_COUNT);
    for index in 0..NODE_COUNT {
        ids.push(graph.add_node(
            "shape",
            SceneRect::new(
                (index % COLUMNS) as f64 * 512.0,
                (index / COLUMNS) as f64 * 512.0,
                32.0,
                32.0,
            ),
            None,
        ));
    }
    assert_eq!(graph.node_count(), NODE_COUNT);
    assert_ne!(ids.last(), Some(&0));
    assert_eq!(graph.hit_test(1.0, 1.0), vec![ids[0]]);
    let build_elapsed = build_started.elapsed();
    let initial_rebuilds = graph.spatial_full_rebuild_count();
    assert_eq!(initial_rebuilds, 1);
    let initial_updates = graph.spatial_incremental_update_count();
    let move_query_started = Instant::now();
    let mut max_candidates = 0;

    for operation in 0..MOVE_QUERY_OPERATIONS {
        let id = ids[(operation * 7_919) % ids.len()];
        graph
            .move_node(id, 300.0, 0.0)
            .expect("generated movement must remain valid");
        let bounds = graph
            .get(id)
            .expect("moved node must remain in the graph")
            .bounds;
        let hits = graph.hit_test(bounds.x + 1.0, bounds.y + 1.0);
        assert!(
            hits.contains(&id),
            "moved node disappeared from hit testing"
        );
        assert_eq!(
            graph.spatial_full_rebuild_count(),
            initial_rebuilds,
            "bounds-only movement unexpectedly rebuilt the full index"
        );
        max_candidates = max_candidates.max(graph.last_spatial_candidate_count());
        black_box(hits);
    }

    let move_query_elapsed = move_query_started.elapsed();
    let incremental_updates = graph.spatial_incremental_update_count() - initial_updates;
    assert_eq!(incremental_updates, MOVE_QUERY_OPERATIONS as u64);
    assert!(
        max_candidates <= 2,
        "unexpected candidate count: {max_candidates}"
    );
    assert!(
        move_query_elapsed <= MOVE_QUERY_BUDGET,
        "100k-node move/query probe took {move_query_elapsed:?}, budget {MOVE_QUERY_BUDGET:?}"
    );

    println!("SceneGraph incremental move/query probe passed");
    println!("nodes: {NODE_COUNT}");
    println!("operations: {MOVE_QUERY_OPERATIONS}");
    println!("initial build/index: {build_elapsed:?}");
    println!("move/query total: {move_query_elapsed:?}");
    println!(
        "move/query average: {} ns/op",
        move_query_elapsed.as_nanos() / MOVE_QUERY_OPERATIONS as u128
    );
    println!("full rebuilds during movement: 0");
    println!("incremental spatial updates: {incremental_updates}");
    println!("maximum query candidates: {max_candidates}");
    println!("budget: {MOVE_QUERY_BUDGET:?}");
}
