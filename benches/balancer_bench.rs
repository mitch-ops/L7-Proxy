use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rust_proxy::balancer::{
    ConsistentHash, LeastConnections, LoadBalancer, RoundRobin, WeightedRoundRobin,
};

fn bench_round_robin(c: &mut Criterion) {
    let mut group = c.benchmark_group("balancer/round_robin");
    for pool_size in [2, 5, 10] {
        let rr = RoundRobin::new();
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            &pool_size,
            |b, &ps| b.iter(|| rr.next_index(black_box(ps))),
        );
    }
    group.finish();
}

fn bench_least_connections(c: &mut Criterion) {
    let mut group = c.benchmark_group("balancer/least_connections");
    for pool_size in [2, 5, 10] {
        let lc = LeastConnections::new(pool_size);
        // Pre-populate some active connections to exercise comparison logic
        lc.record_start(0);
        lc.record_start(0);
        if pool_size > 2 {
            lc.record_start(2);
        }
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            &pool_size,
            |b, &ps| b.iter(|| lc.next_index(black_box(ps))),
        );
    }
    group.finish();
}

fn bench_weighted_round_robin(c: &mut Criterion) {
    let mut group = c.benchmark_group("balancer/weighted_round_robin");

    let configs: Vec<(&str, Vec<u32>)> = vec![
        ("uniform_3", vec![1, 1, 1]),
        ("skewed_3", vec![10, 5, 1]),
        ("large_5", vec![100, 50, 25, 10, 5]),
    ];

    for (name, weights) in &configs {
        let wrr = WeightedRoundRobin::new(weights.clone());
        let pool_size = weights.len();
        group.bench_with_input(BenchmarkId::new("weights", name), &pool_size, |b, &ps| {
            b.iter(|| wrr.next_index(black_box(ps)))
        });
    }
    group.finish();
}

fn bench_consistent_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("balancer/consistent_hash");
    for pool_size in [2, 5, 10] {
        let ch = ConsistentHash::new(pool_size, 150);
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            &pool_size,
            |b, &ps| b.iter(|| ch.next_index_for_key(black_box(ps), black_box("/api/users/42"))),
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_round_robin,
    bench_least_connections,
    bench_weighted_round_robin,
    bench_consistent_hash
);
criterion_main!(benches);
