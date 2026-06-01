# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Nothing yet._

## [0.1.0] — 2026-06-01

Initial public release. Extracted from the `prediction` repository's
`src/pricing/` module (the pure-math modules only), with numerical bug fixes
and a hardened convergence contract applied before publication.

### Added

- Closed-form Black-76 call / put / vega / intrinsic in [`pricing`].
- First-order Greeks — delta, gamma, vega (per 1%), theta (per day), rho
  (per 1%) — in [`greeks`].
- Implied-volatility solver in [`iv_solver`] — Newton–Raphson with a
  Brent's-method fallback when vega is too small (deep OTM / near-expiry).
  `solve_iv` for a single price; `solve_iv_triple` for bid/mid/ask quotes.
- `SolverStatus` enum on `SolverResult` — the precise reason behind a
  (non-)convergence: `Converged`, `NearExpiryIntrinsic`, `NonPositivePrice`,
  `BelowIntrinsic`, `NoBracketInRange`, `NotIdentifiable`, `MaxIterations`,
  `InvalidInput`.
- `BlackInputs` and `IvQuery` — typo-resistant, named-field wrappers over the
  positional free functions, with `const` constructors.
- [`SolverConfig`] (and builder) for tuning iteration counts, tolerances, IV
  bounds, vega floor, and the near-expiry cutoff — including the
  volatility-space `iv_tolerance`.
- `vol-surface` feature: per-expiry [`VolSmile`] with linear-in-strike
  interpolation, flat extrapolation, and quality tiering.
- `digital` feature (requires `vol-surface`): risk-neutral probability
  extraction via call-spread replication and `N(d2)` with skew adjustment.
- `serde` feature: `Serialize` / `Deserialize` derives on the public API.
- `#![forbid(unsafe_code)]`, `#[must_use]` on pure functions and builders, and
  `const fn` builder setters.
- Six runnable examples, two Criterion benches, golden-value vectors
  (including third-party `math.erf` references), and proptest properties
  (parity, IV roundtrip, vega/delta/theta/rho finite-difference, σ-monotonicity,
  no-arbitrage bounds).

### Convergence contract

- Convergence is decided in **volatility space** (`|Δσ| < iv_tolerance` for
  Newton-Raphson; bracket width in `σ` for Brent), not on an absolute price
  residual. This removes false `converged = true` results in vega-weak or
  large-forward regimes and is scale-free (audit **H-2**). The price residual
  is still reported.
- `solve_iv` returns `iv = f64::NAN` on **every** non-converged path and sets
  `converged = false`, with a `SolverStatus` explaining why — including
  `NearExpiryIntrinsic` for the near-expiry cutoff and `InvalidInput` for
  non-finite inputs (audit **H-1**). No silently clamped boundary values
  pretending to be solutions.

### Fixed during extraction (from the upstream `prediction` audit)

- **CRIT-A-01** — `iv_solver`: when Brent's bracket has no sign change, the
  solver previously returned a clamped boundary (`iv = iv_min`/`iv_max`) with
  `converged = false`; consumers ignoring `converged` silently used those
  boundary values. Fixed by returning `iv = f64::NAN` so the failure cannot be
  ignored. Regression: `iv_solver::tests::brent_no_bracket_returns_nan_iv`.
- **HIGH-A-01** — `greeks`: the theta carry term carried the wrong sign
  (`−r · C` instead of `+r · C` per Hull §17.8 eq 17.4 and Haug §1.1.5),
  biasing theta on long options at non-zero rates. Fixed in `compute_greeks`.
  Regressions: `greeks::tests::theta_sign_correct_at_nonzero_rate(_put)`.
- **HIGH-A-02** — `digital`: the call-spread risk-neutral probability was
  computed from the discounted call-price difference without rescaling by
  `exp(rT)`, collapsing toward `0.5 · exp(−rT)` at the ATM strike (~5% bias at
  `r = 5%, T = 1y`). Fixed by dividing by `df = exp(−rT)`. Regression:
  `digital::tests::call_spread_with_nonzero_rate`.
- **negative-time-value gate** — the `solve_iv` lower-bound check compared the
  market price against the *undiscounted* intrinsic `F − K`, rejecting feasible
  ITM prices in `[df·intrinsic, intrinsic)`. Fixed to compare against the
  *discounted* intrinsic. Regressions:
  `iv_solver::tests::itm_{call,put}_between_discounted_and_undiscounted_intrinsic_accepted`.
- **M-1** — `compute_greeks` now guards `sigma <= 0` (mirroring `pricing`),
  returning the intrinsic delta and zero higher-order Greeks instead of `NaN`.
- **M-2** — `digital` call-spread probability is centered on the target strike
  (not the bracket midpoint) and no longer silently clamps an arbitraging smile
  to `[0, 1]` — it returns `None`.
- **L-1 / P-2** — `d1_d2` avoids forming `σ²` (overflow-safe) and uses fused
  multiply-add for tighter rounding. Golden values are unchanged within their
  `1e-12` tolerance.
- `VolSmile::new` excludes non-finite strike/IV observations, so the
  sorted-by-strike invariant always holds and interpolation never returns
  `NaN` on a clean in-range query.
- `digital::extract_probabilities` requires `t > 0`, returning `None` at/after
  expiry instead of leaking a `NaN` probability.

### Documented

- Input preconditions (`F > 0, K > 0`, finite) on the pricing API, asserted in
  debug builds within `d1_d2` (audit **M-4**).
- `vol_surface` interpolation is **not** arbitrage-free; `nearest_bracket` uses
  a scale-relative strike-equality tolerance for crypto-magnitude strikes
  (audit **M-3**, **L-3**).

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
