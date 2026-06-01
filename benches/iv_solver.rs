//! Microbenchmarks for the implied-volatility solver.
//!
//! Covers the common-path (NR converges) and fallback-path (deep OTM ->
//! Brent) scenarios. The market prices are pre-synthesised from a known
//! sigma so the loop body only times the solve, not pricing.
//!
//! ```bash
//! cargo bench --bench iv_solver
//! ```

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use black_76::{SolverConfig, call_price, solve_iv, solve_iv_triple};

fn bench_solve_iv_atm(c: &mut Criterion) {
    let cfg = SolverConfig::default();
    let market = call_price(100.0, 100.0, 1.0, 0.20, 0.05);
    c.bench_function("solve_iv atm (NR path)", |b| {
        b.iter(|| {
            solve_iv(
                black_box(market),
                black_box(100.0),
                black_box(100.0),
                black_box(1.0),
                black_box(0.05),
                black_box(true),
                black_box(&cfg),
            )
        });
    });
}

fn bench_solve_iv_deep_otm(c: &mut Criterion) {
    let cfg = SolverConfig::default();
    let market = call_price(100.0, 200.0, 0.10, 0.20, 0.05);
    c.bench_function("solve_iv deep_otm (likely Brent fallback)", |b| {
        b.iter(|| {
            solve_iv(
                black_box(market),
                black_box(100.0),
                black_box(200.0),
                black_box(0.10),
                black_box(0.05),
                black_box(true),
                black_box(&cfg),
            )
        });
    });
}

fn bench_solve_iv_triple(c: &mut Criterion) {
    let cfg = SolverConfig::default();
    let bid = call_price(100.0, 100.0, 1.0, 0.195, 0.05);
    let mid = call_price(100.0, 100.0, 1.0, 0.200, 0.05);
    let ask = call_price(100.0, 100.0, 1.0, 0.205, 0.05);
    c.bench_function("solve_iv_triple atm", |b| {
        b.iter(|| {
            solve_iv_triple(
                black_box(bid),
                black_box(mid),
                black_box(ask),
                black_box(100.0),
                black_box(100.0),
                black_box(1.0),
                black_box(0.05),
                black_box(true),
                black_box(&cfg),
            )
        });
    });
}

criterion_group!(
    benches,
    bench_solve_iv_atm,
    bench_solve_iv_deep_otm,
    bench_solve_iv_triple,
);
criterion_main!(benches);
