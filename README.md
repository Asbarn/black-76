# black-76

[![Crates.io](https://img.shields.io/crates/v/black-76.svg)](https://crates.io/crates/black-76)
[![Documentation](https://docs.rs/black-76/badge.svg)](https://docs.rs/black-76)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Black-76 closed-form pricing, Greeks, and implied-volatility solver for
European options on **forwards and futures**. Synchronous `f64` math, no
allocations on the hot path, no logging dependency.

The Black-76 model (Fischer Black, *The Pricing of Commodity Contracts*,
Journal of Financial Economics, 1976) prices options whose underlying is a
forward contract rather than a spot asset. Typical applications:
Eurodollar futures options, FX forward options, commodity-futures options,
and crypto perpetual-futures options.

## Features

- Closed-form **call / put prices**, **vega**, and **intrinsic value**.
- First-order **Greeks** — delta, gamma, vega (per 1%), theta (per day), rho (per 1%).
- **Implied-volatility solver** — Newton–Raphson with a Brent's-method
  fallback when vega is too small (deep OTM / near-expiry). Convergence is
  decided in **volatility space**, so it holds at any forward scale.
- Explicit **convergence contract**: a [`SolverResult`] whose `iv` is `NaN`
  (and `converged = false`) on **every** non-converged path, plus a
  [`SolverStatus`] enum giving the exact reason (near-expiry, below intrinsic,
  non-positive price, no root in range, not identifiable, max iterations).
  No silently clamped boundary values pretending to be solutions.
- Optional **per-expiry vol smile** (`vol-surface`) with linear
  interpolation and flat extrapolation.
- Optional **risk-neutral probability extraction** (`digital`) via
  call-spread replication and `N(d2)` with skew adjustment.

Single runtime dependency: `statrs` (for the standard normal CDF / PDF).
No `chrono`, no `tracing`, no async runtime, no `serde` unless you opt in.

## Quick start

```toml
[dependencies]
black-76 = "0.1"
```

```rust
use black_76::{call_price, solve_iv, SolverConfig};

// Price an ATM call: F=100, K=100, T=1 year, σ=20%, r=0
let c = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
assert!((c - 7.9656).abs() < 1e-3);

// Recover the IV from a market price
let cfg = SolverConfig::default();
let result = solve_iv(7.9656, 100.0, 100.0, 1.0, 0.0, true, &cfg);
assert!(result.converged);
assert!((result.iv - 0.20).abs() < 1e-4);
```

### Typo-resistant inputs

Positional `f64` arguments are easy to mis-order. [`BlackInputs`] and
[`IvQuery`] name every field (and are `const`-constructible):

```rust
use black_76::{BlackInputs, IvQuery, SolverConfig};

let inputs = BlackInputs::new(100.0, 100.0, 1.0, 0.20, 0.0); // f, k, t, sigma, r
let c = inputs.call_price();
let g = inputs.greeks(true); // delta, gamma, vega, theta, rho

let iv = IvQuery::new(c, 100.0, 100.0, 1.0, 0.0, true)
    .solve(&SolverConfig::default());
assert!(iv.converged);
```

### Convergence checking is mandatory

```rust
use black_76::{solve_iv, SolverConfig};

let cfg = SolverConfig::default();
let result = solve_iv(/* implausibly high market price */ 1_000.0,
                      100.0, 100.0, 1.0, 0.0, true, &cfg);
if result.converged {
    println!("IV = {}", result.iv);
} else {
    eprintln!("no IV ({:?}) — result.iv is NaN", result.status);
}
```

Skipping the `converged` check is a bug: `iv` is `f64::NAN` on every
non-converged path, and [`SolverStatus`] tells you which one.

## Feature flags

| Flag           | Default? | What it enables                                                   |
|----------------|----------|-------------------------------------------------------------------|
| *(none)*       | yes      | core pricing, Greeks, IV solver                                   |
| `serde`        | no       | `Serialize` / `Deserialize` derives on public types               |
| `vol-surface`  | no       | per-expiry [`VolSmile`] with interpolation                        |
| `digital`      | no       | risk-neutral probability extraction (requires `vol-surface`)      |

```toml
black-76 = { version = "0.1", features = ["vol-surface", "digital", "serde"] }
```

## Examples

Six runnable examples under `examples/`:

```bash
cargo run --example atm_price
cargo run --example solve_iv
cargo run --example put_call_parity
cargo run --example greeks
cargo run --example vol_smile     --features vol-surface
cargo run --example digital_prob  --features digital
```

## Benchmarks

Two Criterion benches under `benches/`:

```bash
cargo bench --bench pricing
cargo bench --bench iv_solver
```

## Conventions

- **Vega** is reported per 1% absolute change in IV (trader convention),
  i.e. the raw `dC/dσ` divided by 100.
- **Theta** is reported per calendar day with `year = 365.25` days.
- **Time** is in years.
- **Rate** is continuous compounding.
- All numerics are `f64`. The crate exposes no `Decimal`-typed APIs.

## Comparison with alternatives

| | `black-76` | `py_vollib` | QuantLib (bindings) | `RustQuant` / `black_scholes` |
|---|---|---|---|---|
| Language | pure Rust | Python + C ext | C++ via FFI | Rust |
| Runtime deps | `statrs` only | NumPy/SciPy | QuantLib | varies |
| Async / runtime | none | — | — | none |
| Model focus | Black-76 (forwards/futures) | Black / BS / -76 | everything | mostly Black-Scholes |
| IV solver | Newton + Brent, **typed `SolverStatus`** | Newton / `lets_be_rational` | various | varies |
| Non-convergence | `iv = NaN` + status enum | exceptions | exceptions | varies by crate |
| Greeks | delta, gamma, vega, theta, rho | full | full | crate-dependent |
| Smile / digitals | optional (`vol-surface`, `digital`) | — | full surfaces | — |
| `forbid(unsafe_code)` | yes | n/a | no (FFI) | varies |
| MSRV / semver | 1.85, `#[non_exhaustive]` API | n/a | n/a | varies |

<sub>Comparison reflects general positioning rather than pinned versions; verify
the specifics against each project's current release.</sub>

`black-76` is intentionally **small and sharp**: a synchronous, dependency-light
forward-options pricer with a rigorous, typed convergence contract — not a full
derivatives library. Reach for QuantLib when you need exotic products or full
curve/surface machinery.

## MSRV

Rust **1.85** (Edition 2024).

## References

- Hull, *Options, Futures, and Other Derivatives*, 10th ed., §17.
- Haug, *The Complete Guide to Option Pricing Formulas*, 2nd ed., §1.1.
- Brent, *Algorithms for Minimization without Derivatives*, 1973, Ch. 4.
- Brenner & Subrahmanyam, *A Simple Formula to Compute the Implied
  Standard Deviation*, Financial Analysts Journal, 1988.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([`LICENSE-MIT`](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual-licensed as above, without any additional terms or
conditions.

[`SolverResult`]: https://docs.rs/black-76/latest/black_76/struct.SolverResult.html
[`SolverStatus`]: https://docs.rs/black-76/latest/black_76/enum.SolverStatus.html
[`BlackInputs`]: https://docs.rs/black-76/latest/black_76/struct.BlackInputs.html
[`IvQuery`]: https://docs.rs/black-76/latest/black_76/struct.IvQuery.html
[`VolSmile`]: https://docs.rs/black-76/latest/black_76/vol_surface/struct.VolSmile.html
