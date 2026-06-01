# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Nothing yet._

## [0.1.0] - 2026-06-01

Initial public release. The pricing core was extracted from a private
crypto-options project and generalized into a standalone, dependency-light
`f64` crate.

### Added

- Closed-form Black-76 call / put / vega / intrinsic in [`pricing`].
- First-order Greeks (delta, gamma, vega per 1%, theta per day, rho per 1%)
  in [`greeks`].
- Implied-volatility solver in [`iv_solver`]: Newton-Raphson with a
  Brent's-method fallback when vega is too small (deep OTM / near-expiry).
  `solve_iv` for a single price; `solve_iv_triple` for bid/mid/ask quotes.
- `SolverStatus` enum on `SolverResult` giving the precise outcome:
  `Converged`, `NearExpiryIntrinsic`, `NonPositivePrice`, `BelowIntrinsic`,
  `NoBracketInRange`, `NotIdentifiable`, `MaxIterations`, `InvalidInput`.
- `BlackInputs` and `IvQuery`: typo-resistant, named-field wrappers over the
  positional free functions, with `const` constructors.
- [`SolverConfig`] (and builder) for tuning iteration counts, tolerances, IV
  bounds, vega floor, the near-expiry cutoff, and the volatility-space
  `iv_tolerance`.
- `vol-surface` feature: per-expiry [`VolSmile`] with linear-in-strike
  interpolation, flat extrapolation, and quality tiering.
- `digital` feature (requires `vol-surface`): risk-neutral probability
  extraction via call-spread replication and `N(d2)` with skew adjustment.
- `serde` feature: `Serialize` / `Deserialize` derives on the public API.
- `#![forbid(unsafe_code)]`, `#[must_use]` on pure functions and builders, and
  `const fn` builder setters.
- Six runnable examples, two Criterion benches, golden-value vectors (including
  third-party `math.erf` and `py_vollib` references), and proptest properties
  (parity, IV roundtrip, vega/delta/theta/rho finite-difference,
  sigma-monotonicity, no-arbitrage bounds).

### Convergence contract

- Convergence is decided in volatility space (`|delta-sigma| < iv_tolerance`
  for Newton-Raphson; bracket width in sigma for Brent), not on an absolute
  price residual. This avoids false `converged = true` results in vega-weak or
  large-forward regimes and is scale-invariant. The price residual is still
  reported.
- `solve_iv` returns `iv = f64::NAN` on every non-converged path and sets
  `converged = false`, with a `SolverStatus` explaining why (including
  `NearExpiryIntrinsic` for the near-expiry cutoff and `InvalidInput` for
  non-finite inputs). No clamped boundary values are reported as solutions.
- The negative-time-value gate compares the market price against the
  *discounted* intrinsic, so feasible ITM prices in `[df*intrinsic, intrinsic)`
  at non-zero rates are accepted.

### Notes

- `compute_greeks` guards `sigma <= 0` (mirroring `pricing`), returning the
  intrinsic delta and zero higher-order Greeks instead of NaN.
- `d1_d2` avoids forming `sigma^2` (overflow-safe) and uses a fused multiply-add
  for tighter rounding.
- `VolSmile::new` drops non-finite (strike, IV) observations so the
  sorted-by-strike invariant always holds and interpolation never returns NaN on
  a clean in-range query. `digital::extract_probabilities` requires `t > 0`,
  returning `None` at/after expiry rather than leaking a NaN probability.
- `vol_surface` interpolation is **not** arbitrage-free; see the module docs.
  Input preconditions (`F > 0`, `K > 0`, finite) are documented on the pricing
  API and asserted in debug builds.

### Decisions

- Edition 2024, MSRV 1.85.
- Single required dependency: `statrs` 0.18 (`default-features = false`).
- `no_std` deferred to a future minor.
- No async, no logging, no `chrono`.
- All numerics are `f64`; no `Decimal`-typed APIs.
- Public types that may grow new variants / fields are marked
  `#[non_exhaustive]`.

[Unreleased]: https://github.com/Asbarn/black-76/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Asbarn/black-76/releases/tag/v0.1.0

[`pricing`]: https://docs.rs/black-76/latest/black_76/pricing/
[`iv_solver`]: https://docs.rs/black-76/latest/black_76/iv_solver/
[`greeks`]: https://docs.rs/black-76/latest/black_76/greeks/
[`SolverConfig`]: https://docs.rs/black-76/latest/black_76/struct.SolverConfig.html
[`VolSmile`]: https://docs.rs/black-76/latest/black_76/vol_surface/struct.VolSmile.html
