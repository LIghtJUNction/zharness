use std::{hint::black_box, time::Instant};

use zharness::tools::{Tool, ToolSet};

const ITERATIONS: usize = 20_000;
const SCHEMA: &str = r#"{"type":"object"}"#;

fn percentile(sorted: &[u128], numerator: usize) -> u128 {
    sorted[((sorted.len() - 1) * numerator) / 100]
}

fn main() {
    for count in [1usize, 4, 16, 64, 256, 1024] {
        let names: Vec<String> = (0..count).map(|i| format!("tool-{i}")).collect();
        let descriptors: Vec<Tool<'_>> = names
            .iter()
            .map(|name| Tool {
                name,
                description: "benchmark tool",
                input_schema: SCHEMA,
            })
            .collect();
        let registry = ToolSet::new(&descriptors).expect("valid registry");
        let target = names.last().expect("nonempty registry").as_str();
        let mut samples = Vec::with_capacity(ITERATIONS);

        for _ in 0..ITERATIONS {
            let start = Instant::now();
            black_box(registry.get(black_box(target)));
            samples.push(start.elapsed().as_nanos());
        }
        samples.sort_unstable();

        let p50 = percentile(&samples, 50);
        let p95 = percentile(&samples, 95);
        let p99 = percentile(&samples, 99);
        let throughput = 1_000_000_000u128.checked_div(p50).unwrap_or(0);

        println!(
            "tools={count}, p50_ns={p50}, p95_ns={p95}, p99_ns={p99},              throughput_ops_s={throughput}, registry_bytes={}",
            descriptors.len() * std::mem::size_of::<Tool<'_>>()
        );
    }
}
