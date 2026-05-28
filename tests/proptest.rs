//! Property-based tests for Black-76 pricing, Greeks, and IV solver.
//!
//! Properties exercised:
//!
//! 1. **Put-call parity**: `C − P = exp(−rT) · (F − K)` for any well-formed
//!    input.
//! 2. **IV roundtrip**: solving for IV from a synthesised market price
//!    recovers the original sigma within solver tolerance.
//! 3. **Vega FD cross-check**: analytic vega agrees with a central finite
//!    difference of the call price.
//! 4. **Monotonicity in σ**: call and put prices are non-decreasing in
//!    volatility.
//! 5. **Bounds**: prices lie within their no-arbitrage envelopes
//!    `[intrinsic_discounted, max_value_discounted]`.
//!
//! Strategies cover ATM and well-conditioned wings, deliberately avoiding
//! deep-OTM corners where vega → 0 and the IV roundtrip can fail Newton
//! convergence (handled by the Brent fallback but with looser tolerance —
//! tested separately in the unit tests).

use proptest::prelude::*;

use black_76::{
    SolverConfig, call_price, compute_greeks, intrinsic_value, put_price, solve_iv, vega,
};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Forward in `[10, 10_000]`.
fn forward() -> impl Strategy<Value = f64> {
    10.0_f64..10_000.0_f64
}

/// Time to expiry in years, `[0.05, 3.0]`.
fn time() -> impl Strategy<Value = f64> {
    0.05_f64..3.0_f64
}

/// Volatility in `[0.05, 2.0]` (5%–200%).
fn sigma() -> impl Strategy<Value = f64> {
    0.05_f64..2.0_f64
}

/// Rate in `[−0.05, 0.20]`.
fn rate() -> impl Strategy<Value = f64> {
    -0.05_f64..0.20_f64
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    /// Put-call parity must hold to ~machine precision.
    #[test]
    fn prop_put_call_parity(
        f in forward(),
        t in time(),
        s in sigma(),
        r in rate(),
    ) {
        let k = f; // ATM is sufficient; parity is strike-agnostic
        let c = call_price(f, k, t, s, r);
        let p = put_price(f, k, t, s, r);
        let df = (-r * t).exp();
        let lhs = c - p;
        let rhs = df * (f - k);
        prop_assert!(
            (lhs - rhs).abs() < 1e-8 * (1.0 + f.abs()),
            "parity violated: C − P = {lhs}, df·(F − K) = {rhs} (F={f}, T={t}, σ={s}, r={r})",
        );

        // Repeat with a non-trivial strike.
        let k2 = f * 1.10;
        let c2 = call_price(f, k2, t, s, r);
        let p2 = put_price(f, k2, t, s, r);
        let lhs2 = c2 - p2;
        let rhs2 = df * (f - k2);
        prop_assert!(
            (lhs2 - rhs2).abs() < 1e-8 * (1.0 + f.abs()),
            "OTM parity violated: C − P = {lhs2}, df·(F − K) = {rhs2}",
        );
    }

    /// IV solved from a synthesised market price recovers the original σ.
    ///
    /// The contract is *price tolerance*, not IV tolerance — the solver
    /// converges when `|model_price − market_price| < price_tolerance`. In
    /// vega-weak regions several IVs can map to nearly the same price, so
    /// the IV residual can be larger than you'd expect even when the price
    /// residual is below tolerance.
    #[test]
    fn prop_iv_roundtrip_call(
        f in forward(),
        m in 0.85_f64..1.15_f64,
        t in 0.10_f64..2.0_f64,
        s in 0.10_f64..1.0_f64,
        r in -0.02_f64..0.10_f64,
    ) {
        let k = f * m;
        let market = call_price(f, k, t, s, r);

        // Skip vanishing-time-value cases (`market` − discounted intrinsic
        // ≈ 0, where vega → 0 and the inverse is numerically ill-defined).
        let df = (-r * t).exp();
        let discounted_intrinsic = df * (f - k).max(0.0);
        prop_assume!(market - discounted_intrinsic > 0.01 * f);

        let cfg = SolverConfig::default();
        let result = solve_iv(market, f, k, t, r, true, &cfg);

        prop_assert!(
            result.converged,
            "solver failed to converge: F={f}, K={k}, T={t}, σ={s}, r={r}, market={market}, result={result:?}",
        );
        prop_assert!(
            result.residual < cfg.price_tolerance * 10.0,
            "price residual {} not within 10x tolerance {}",
            result.residual, cfg.price_tolerance,
        );
        prop_assert!(
            (result.iv - s).abs() < 1e-2,
            "IV diverged by more than 1e-2: solved {} vs true {s}, residual {}",
            result.iv, result.residual,
        );
    }

    /// Central finite-difference vega agrees with the analytic value.
    #[test]
    fn prop_vega_finite_difference(
        f in forward(),
        m in 0.6_f64..1.6_f64,
        t in time(),
        s in 0.10_f64..1.0_f64,
        r in rate(),
    ) {
        let k = f * m;
        let h = 1e-5_f64;
        let c_up = call_price(f, k, t, s + h, r);
        let c_dn = call_price(f, k, t, s - h, r);
        let fd = (c_up - c_dn) / (2.0 * h);
        let analytic = vega(f, k, t, s, r);

        // Vega scales with F; use a price-relative tolerance.
        let scale = (analytic.abs() + f).max(1.0);
        prop_assert!(
            (fd - analytic).abs() < 1e-3 * scale,
            "vega FD={fd}, analytic={analytic} (F={f}, K={k}, T={t}, σ={s}, r={r})",
        );

        // The per-1% Greek is exactly analytic/100.
        let g = compute_greeks(f, k, t, s, r, true);
        prop_assert!(
            (g.vega - analytic / 100.0).abs() < 1e-12 * scale,
            "Greek vega-per-1% disagrees with raw vega / 100",
        );
    }

    /// Both call and put prices are non-decreasing in σ.
    #[test]
    fn prop_monotonic_in_sigma(
        f in forward(),
        m in 0.5_f64..2.0_f64,
        t in time(),
        s1 in 0.05_f64..0.5_f64,
        delta in 0.01_f64..0.5_f64,
        r in rate(),
    ) {
        let k = f * m;
        let s2 = s1 + delta;
        let c1 = call_price(f, k, t, s1, r);
        let c2 = call_price(f, k, t, s2, r);
        let p1 = put_price(f, k, t, s1, r);
        let p2 = put_price(f, k, t, s2, r);

        // Allow a tiny floating-point slack proportional to the price.
        let tol_c = 1e-10 * (c1.abs() + c2.abs() + 1.0);
        let tol_p = 1e-10 * (p1.abs() + p2.abs() + 1.0);
        prop_assert!(
            c2 + tol_c >= c1,
            "call not monotonic: σ={s1}→{c1}, σ={s2}→{c2}",
        );
        prop_assert!(
            p2 + tol_p >= p1,
            "put not monotonic: σ={s1}→{p1}, σ={s2}→{p2}",
        );
    }

    /// Prices stay within their no-arbitrage envelopes.
    #[test]
    fn prop_price_bounds(
        f in forward(),
        m in 0.5_f64..2.0_f64,
        t in time(),
        s in sigma(),
        r in rate(),
    ) {
        let k = f * m;
        let df = (-r * t).exp();

        let c = call_price(f, k, t, s, r);
        let p = put_price(f, k, t, s, r);
        let call_intrinsic = intrinsic_value(f, k, true) * df;
        let put_intrinsic = intrinsic_value(f, k, false) * df;

        let tol = 1e-10 * (f + 1.0);

        // Call: discounted intrinsic ≤ C ≤ df · F
        prop_assert!(
            c + tol >= call_intrinsic,
            "call below discounted intrinsic: C={c}, intrinsic*df={call_intrinsic}",
        );
        prop_assert!(
            c <= df * f + tol,
            "call above df·F: C={c}, df·F={}",
            df * f,
        );

        // Put: discounted intrinsic ≤ P ≤ df · K
        prop_assert!(
            p + tol >= put_intrinsic,
            "put below discounted intrinsic: P={p}, intrinsic*df={put_intrinsic}",
        );
        prop_assert!(
            p <= df * k + tol,
            "put above df·K: P={p}, df·K={}",
            df * k,
        );

        // Trivial positivity.
        prop_assert!(c >= -tol, "call must be non-negative");
        prop_assert!(p >= -tol, "put must be non-negative");
    }
}
