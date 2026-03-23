use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_proxy::balancer::RoundRobin;
use rust_proxy::router::{Route, Router};

fn make_route(prefix: &str) -> Route {
    Route {
        prefix: prefix.to_string(),
        upstreams: vec!["http://127.0.0.1:9999".to_string()],
        balancer: Box::new(RoundRobin::new()),
    }
}

fn bench_router(c: &mut Criterion) {
    let mut group = c.benchmark_group("router_match");

    // Few routes — typical case
    let router = Router::new(vec![
        make_route("/api"),
        make_route("/api/v2"),
        make_route("/admin"),
        make_route("/health"),
        make_route("/static"),
    ]);
    group.bench_function("few_routes", |b| {
        b.iter(|| router.match_route(black_box("/api/v2/users")))
    });

    // Many routes — scaling behavior
    let routes: Vec<Route> = (0..50).map(|i| make_route(&format!("/route-{:02}", i))).collect();
    let big_router = Router::new(routes);
    group.bench_function("many_routes", |b| {
        b.iter(|| big_router.match_route(black_box("/route-45/subpath")))
    });

    // No match — miss path
    let miss_router = Router::new(
        (0..10).map(|i| make_route(&format!("/svc-{}", i))).collect(),
    );
    group.bench_function("no_match", |b| {
        b.iter(|| miss_router.match_route(black_box("/unknown/path")))
    });

    // Deep prefix — longest prefix matching depth
    let deep_router = Router::new(vec![
        make_route("/a"),
        make_route("/a/b"),
        make_route("/a/b/c"),
        make_route("/a/b/c/d"),
    ]);
    group.bench_function("deep_prefix", |b| {
        b.iter(|| deep_router.match_route(black_box("/a/b/c/d/e")))
    });

    group.finish();
}

criterion_group!(benches, bench_router);
criterion_main!(benches);
