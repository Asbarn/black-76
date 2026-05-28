# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-05-28

Initial public release. Extracted from the `prediction` repository's
`src/pricing/` module with three pre-publish bug fixes applied during
extraction (see below).

### Added

- Closed-form Black-76 call / put / vega / intrinsic in [`pricing`].
- Implied-volatility solver in [`iv_solver`] — Newton–Raphson with Brent's-method
  fallback when vega is too small (deep OTM / near-expiry).
  - `solve_iv` for a single price; `solve_iv_triple` for bid/mid/ask quotes.
- First-order Greeks (delta, vega-per-1%, theta-per-day) in [`greeks`].
- [`SolverConfig`] (and builder) for tuning iteration counts, tolerances,
  IV bounds, vega floor, and the near-expiry cutoff.
- `vol-surface` feature: per-expiry [`VolSmile`] with linear-in-strike
  interpolation, flat extrapolation, and quality tiering.
- `digital` feature (requires `vol-surface`): risk-neutral probability
  extraction via call-spread replication and `N(d2)` with skew adjustment.
- `serde` feature: `Serialize` / `Deserialize` derives on the public API.
- Six runnable examples, two Criterion benches, golden-value test vector,
  and five proptest properties (parity, IV roundtrip, vega FD,
  monotonicity, no-arbitrage bounds).

### Fixed (pre-publish, from the upstream `prediction` repo's audit)

- **CRIT-A-01** — `iv_solver`: when Brent's bracket has no sign change, the
  solver previously returned a clamped boundary with `converged = false`
  but `iv = iv_min` or `iv = iv_max`. Consumers that ignored `converged`
  silently used those boundary values as solutions. Fixed by returning
  `iv = f64::NAN` together with `converged = false` so the failure cannot
  be ignored. Regression: `iv_solver::tests::brent_no_bracket_returns_nan_iv`.
- **HIGH-A-01** — `greeks`: the theta carry term carried the wrong sign
  (`−r · C` instead of `+r · C` per Hull §17.8 eq 17.4 and Haug §1.1.5).
  Theta on long options at non-zero rates was therefore biased. Fixed in
  `compute_greeks`. Regression: `greeks::tests::theta_sign_correct_at_nonzero_rate`
  (and `_put`).
- **HIGH-A-02** — `digital`: the call-spread risk-neutral probability was
  computed from the discounted call-price difference without rescaling by
  `exp(rT)`, so the result silently collapsed toward `0.5 · exp(−rT)` at
  the ATM strike (a ~5% bias at `r = 5%, T = 1y`). Fixed by dividing by
  `df = exp(−rT)` in `call_spread_probability`. Regression:
  `digital::tests::call_spread_with_nonzero_rate`.

### Fixed (discovered during proptest hardening)

- **`solve_iv` negative-time-value gate** — the gate at the top of
  `solve_iv` compared the market price against the *undiscounted*
  intrinsic `F − K`, but the true Black-76 no-arbitrage lower bound for an
  ITM call is the *discounted* intrinsic `exp(−rT) · (F − K)` (and
  symmetrically for puts). ITM prices in `[df·intrinsic, intrinsic)` were
  legitimate Black-76 outputs but got rejected with
  `iv = iv_min, converged = false`. Fixed by comparing against
  discounted intrinsic. Regressions:
  `iv_solver::tests::itm_call_between_discounted_and_undiscounted_intrinsic_accepted`
  and `..._put_...` (plus full ITM moneyness coverage in the
  `prop_iv_roundtrip_call` proptest).

### Decisions documented for v0.1

- Edition 2024, MSRV 1.85.
- Single required dependency: `statrs` 0.18 (`default-features = false`).
- `no_std` deferred to a future minor.
- No async, no logging, no `chrono`.
- All numerics are `f64`; no `Decimal`-typed APIs.
- Public types that may grow new variants / fields in a future minor
  version are marked `#[non_exhaustive]`.

[Unreleased]: https://github.com/Asbarn/black-76/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Asbarn/black-76/releases/tag/v0.1.0

[`pricing`]: https://docs.rs/black-76/latest/black_76/pricing/
[`iv_solver`]: https://docs.rs/black-76/latest/black_76/iv_solver/
[`greeks`]: https://docs.rs/black-76/latest/black_76/greeks/
[`SolverConfig`]: https://docs.rs/black-76/latest/black_76/struct.SolverConfig.html
[`VolSmile`]: https://docs.rs/black-76/latest/black_76/vol_surface/struct.VolSmile.html
