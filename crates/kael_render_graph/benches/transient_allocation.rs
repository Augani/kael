//! Measures graph allocation planning, independent of GPU submission and graph compilation.

use kael_render_graph::{CompiledGraph, PassDesc, RenderGraph, ResourceDesc};
use std::{hint::black_box, time::Instant};

const RESOURCE_COUNT: usize = 30_000;
const SAMPLES: usize = 9;

fn overlapping_graph() -> CompiledGraph {
    let mut graph = RenderGraph::new();
    let mut producer = PassDesc::new("produce");
    let mut consumer = PassDesc::new("consume");
    for index in 0..RESOURCE_COUNT {
        let resource = graph.add_resource(ResourceDesc::transient_texture(format!("r{index}")));
        producer.writes.push(resource);
        consumer.reads.push(resource);
    }
    graph.add_pass(producer);
    graph.add_pass(consumer);
    graph.compile().unwrap()
}

fn sequential_graph(class_count: u64) -> CompiledGraph {
    let mut graph = RenderGraph::new();
    for index in 0..RESOURCE_COUNT {
        let resource = graph.add_resource(
            ResourceDesc::transient_texture(format!("r{index}"))
                .allocation_class(index as u64 % class_count),
        );
        graph.add_pass(PassDesc::new(format!("p{index}")).write(resource));
    }
    graph.compile().unwrap()
}

fn measure(name: &str, graph: &CompiledGraph, expected_slots: usize) {
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let allocation = black_box(graph).assign_transient_memory();
        samples.push(start.elapsed());
        assert_eq!(allocation.slot_count, expected_slots);
        assert_eq!(allocation.slot_of.iter().flatten().count(), RESOURCE_COUNT);
        black_box(allocation);
    }
    samples.sort_unstable();
    println!(
        "{name}: resources={RESOURCE_COUNT} slots={expected_slots} median_ms={:.3} min_ms={:.3}",
        samples[SAMPLES / 2].as_secs_f64() * 1000.0,
        samples[0].as_secs_f64() * 1000.0,
    );
}

fn main() {
    measure("overlapping", &overlapping_graph(), RESOURCE_COUNT);
    measure("sequential", &sequential_graph(1), 1);
    measure("mixed_classes", &sequential_graph(256), 256);
}
