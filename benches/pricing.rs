//! Microbenchmarks for closed-form pricing and vega.
//!
//! ```bash
//! cargo bench --bench pricing
//! ```

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use black_76::{call_price, d1_d2, put_price, vega};

fn bench_d1_d2(c: &mut Criterion) {
    c.bench_function("d1_d2", |b| {
        b.iter(|| {
            d1_d2(
                black_box(100.0),
                black_box(105.0),
                black_box(1.0),
                black_box(0.20),
            )
        });
    });
}

fn bench_call_price(c: &mut Criterion) {
    c.bench_function("call_price atm", |b| {
        b.iter(|| {
            call_price(
                black_box(100.0),
                black_box(100.0),
                black_box(1.0),
                black_box(0.20),
                black_box(0.05),
            )
        });
    });

    c.bench_function("call_price deep_otm", |b| {
        b.iter(|| {
            call_price(
                black_box(100.0),
                black_box(150.0),
                black_box(0.10),
                black_box(0.20),
                black_box(0.05),
            )
        });
    });
}

fn bench_put_price(c: &mut Criterion) {
    c.bench_function("put_price atm", |b| {
        b.iter(|| {
            put_price(
                black_box(100.0),
                black_box(100.0),
                black_box(1.0),
                black_box(0.20),
                black_box(0.05),
            )
        });
    });
}

fn bench_vega(c: &mut Criterion) {
    c.bench_function("vega atm", |b| {
        b.iter(|| {
            vega(
                black_box(100.0),
                black_box(100.0),
                black_box(1.0),
                black_box(0.20),
                black_box(0.05),
            )
        });
    });
}

criterion_group!(
    benches,
    bench_d1_d2,
    bench_call_price,
    bench_put_price,
    bench_vega
);
criterion_main!(benches);
