//! Implied volatility solver with Newton-Raphson + Brent fallback.
//!
//! Solves for `σ` such that `Black-76 price(σ) == market_price` within a
//! configurable tolerance.
//!
//! ## Convergence checking
//!
//! Callers **must** check [`SolverResult::converged`] (or
//! [`SolverResult::status`]) before consuming `iv`. Whenever the solver does
//! not converge, `iv` is [`f64::NAN`] and [`SolverStatus`] reports why.
//!
//! Convergence is decided in **volatility space** — the Newton step (or the
//! Brent bracket width) in `σ` falls below `iv_tolerance` — rather than on an
//! absolute price residual. An absolute price tolerance is not a valid
//! criterion where vega is small or the forward is large: the price is then
//! flat in `σ`, so a price-based test can declare success at the wrong `σ`
//! (audit H-2). The `residual` field still reports `|model_price − market_price|`.
//!
//! ## Edge cases ([`SolverStatus`])
//!
//! - **Near-expiry** (`t < near_expiry_cutoff_hours`): `iv = NaN`,
//!   `converged = false`, [`SolverStatus::NearExpiryIntrinsic`] — price the
//!   option intrinsically yourself.
//! - **Zero/negative price**: [`SolverStatus::NonPositivePrice`].
//! - **Below discounted intrinsic**: [`SolverStatus::BelowIntrinsic`].
//! - **No root in `[iv_min, iv_max]`**: [`SolverStatus::NoBracketInRange`].
//! - **Vega below floor** (deep OTM/ITM): NR falls back to Brent; if the
//!   price is flat in `σ`, the result is [`SolverStatus::NotIdentifiable`].
//! - **Iteration budget exhausted**: [`SolverStatus::MaxIterations`].

use crate::config::SolverConfig;
use crate::pricing;
use crate::types::{SolverMethod, SolverResult, SolverStatus};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HOURS_PER_YEAR: f64 = 8760.0;
const TWO_PI: f64 = std::f64::consts::TAU;

// ---------------------------------------------------------------------------
// Initial guess: Brenner-Subrahmanyam approximation
// ---------------------------------------------------------------------------

/// Brenner-Subrahmanyam approximation for the initial IV guess.
///
/// `sigma_0 = sqrt(2π/T) · (C/F)`.
///
/// Works well for near-ATM options. For deep OTM/ITM the approximation can
/// be poor, but the NR loop corrects quickly when vega is healthy.
/// Clamped to `[iv_min, iv_max]`.
fn brenner_subrahmanyam_guess(market_price: f64, f: f64, t: f64, iv_min: f64, iv_max: f64) -> f64 {
    if f <= 0.0 || t <= 0.0 {
        return f64::midpoint(iv_min, iv_max);
    }
    let sigma_0 = (TWO_PI / t).sqrt() * (market_price / f);
    sigma_0.clamp(iv_min, iv_max)
}

// ---------------------------------------------------------------------------
// Brent's method (bracketed root-finding)
// ---------------------------------------------------------------------------

/// Brent's method.
///
/// Finds `σ ∈ [iv_min, iv_max]` such that `price(σ) − market_price = 0`.
/// Combines inverse quadratic interpolation, secant, and bisection per
/// Brent (1973), Ch. 4.
fn brent_solve(
    market_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> SolverResult {
    let objective =
        |sigma: f64| -> f64 { pricing::price(f, k, t, sigma, r, is_call) - market_price };

    let mut a = config.iv_min;
    let mut b = config.iv_max;
    let mut fa = objective(a);
    let mut fb = objective(b);

    // No sign change in bracket: no root exists in [iv_min, iv_max].
    //
    // Per audit CRIT-A-01: the original implementation returned the endpoint
    // with smaller residual as if it were the answer, with converged based
    // on residual < tolerance. That contaminated downstream consumers that
    // didn't check `converged`. Fix: explicitly return NaN with converged = false
    // and let callers handle the unsolvable case.
    if fa * fb > 0.0 {
        let best_residual = fa.abs().min(fb.abs());
        return SolverResult {
            iv: f64::NAN,
            method: SolverMethod::Brent,
            iterations: 0,
            converged: false,
            status: SolverStatus::NoBracketInRange,
            residual: best_residual,
        };
    }

    // Ensure |f(a)| >= |f(b)| so b is the current best approximation.
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }

    let mut c = a;
    let mut fc = fa;
    let mut mflag = true;
    let mut d = b - a; // previous step

    for i in 0..config.brent_max_iterations {
        // Converged: the bracket is tight in volatility space. A small σ
        // bracket means we have pinned the root; this is scale-free and
        // cannot report a flat-price region as converged (audit H-2).
        if (b - a).abs() < config.iv_tolerance {
            return SolverResult {
                iv: b,
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged: true,
                status: SolverStatus::Converged,
                residual: fb.abs(),
            };
        }

        // Bracket collapsed to machine precision before reaching iv_tolerance.
        // Accept only if the price residual is also tiny; otherwise the price
        // is flat in σ here and the implied volatility is not identifiable.
        if (b - a).abs() < f64::EPSILON * (a.abs() + b.abs()).max(1.0) {
            let converged = fb.abs() < config.price_tolerance;
            return SolverResult {
                iv: if converged { b } else { f64::NAN },
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged,
                status: if converged {
                    SolverStatus::Converged
                } else {
                    SolverStatus::NotIdentifiable
                },
                residual: fb.abs(),
            };
        }

        // Inverse quadratic interpolation if all three function values differ.
        let s = if (fa - fc).abs() > f64::EPSILON && (fb - fc).abs() > f64::EPSILON {
            a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb))
        } else {
            // Fall back to secant.
            b - fb * (b - a) / (fb - fa)
        };

        // Reject interpolation when conditions for safe acceptance fail.
        let midpoint = f64::midpoint(a, b);
        let reject_interpolation = {
            let bound1 = 3.0_f64.mul_add(a, b) / 4.0;
            let (lo, hi) = if bound1 < b { (bound1, b) } else { (b, bound1) };
            s < lo || s > hi
        } || (mflag && (s - b).abs() >= (b - c).abs() / 2.0)
            || (!mflag && (s - b).abs() >= (c - d).abs() / 2.0)
            || (mflag && (b - c).abs() < 1e-15)
            || (!mflag && (c - d).abs() < 1e-15);

        let s = if reject_interpolation {
            mflag = true;
            midpoint
        } else {
            mflag = false;
            s
        };

        let fs = objective(s);
        d = c;
        c = b;
        fc = fb;

        if fa * fs < 0.0 {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }

        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }

    // Hit max iterations without tightening the σ bracket below iv_tolerance.
    SolverResult {
        iv: f64::NAN,
        method: SolverMethod::Brent,
        iterations: config.brent_max_iterations,
        converged: false,
        status: SolverStatus::MaxIterations,
        residual: fb.abs(),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Solves for implied volatility given a market price.
///
/// Uses Newton-Raphson with Brent's-method fallback when vega is too small
/// for NR to converge reliably.
///
/// # Convergence checking
///
/// **Callers must check `result.converged` before consuming `result.iv`.**
/// When the market price is outside the feasible Black-76 range, `iv` will
/// be [`f64::NAN`] and `converged` will be `false`.
///
/// # Parameters
///
/// - `market_price`: observed option price.
/// - `f`: forward / futures price.
/// - `k`: strike.
/// - `t`: time to expiry in years.
/// - `r`: risk-free rate (annualized, continuously compounded).
/// - `is_call`: `true` for call, `false` for put.
/// - `config`: solver configuration ([`SolverConfig`]).
///
/// # Returns
///
/// A [`SolverResult`] with IV, convergence status, method used, iteration
/// count, and residual.
///
/// # Examples
///
/// ```
/// use black_76::{call_price, solve_iv, SolverConfig};
///
/// let cfg = SolverConfig::default();
/// // Price an ATM call and solve back for IV.
/// let price = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
/// let result = solve_iv(price, 100.0, 100.0, 1.0, 0.0, true, &cfg);
/// assert!(result.converged);
/// assert!((result.iv - 0.20).abs() < 1e-6);
/// ```
#[must_use]
pub fn solve_iv(
    market_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> SolverResult {
    // 1. Near-expiry cutoff: bypass solver and return intrinsic-pricing sentinel.
    let near_expiry_cutoff_years = config.near_expiry_cutoff_hours / HOURS_PER_YEAR;
    if t <= 0.0 || t < near_expiry_cutoff_years {
        return SolverResult {
            iv: f64::NAN,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            status: SolverStatus::NearExpiryIntrinsic,
            residual: 0.0,
        };
    }

    // 2. Zero or negative market price: no valid IV exists.
    if market_price <= 0.0 {
        return SolverResult {
            iv: f64::NAN,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            status: SolverStatus::NonPositivePrice,
            residual: market_price.abs(),
        };
    }

    // 3. Negative time value: `market_price` below the discounted intrinsic
    //    `df · max(0, F − K)` for calls (or `df · max(0, K − F)` for puts).
    //    Comparing against undiscounted intrinsic is wrong at non-zero rates
    //    — the true Black-76 no-arbitrage lower bound for an ITM call is the
    //    *discounted* intrinsic, and prices in `[df·intrinsic, intrinsic)`
    //    are perfectly feasible.
    let intrinsic = pricing::intrinsic_value(f, k, is_call);
    let df = (-r * t).exp();
    let discounted_intrinsic = df * intrinsic;
    if market_price < discounted_intrinsic {
        return SolverResult {
            iv: f64::NAN,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            status: SolverStatus::BelowIntrinsic,
            residual: (discounted_intrinsic - market_price).abs(),
        };
    }

    // 4. Compute initial guess via Brenner-Subrahmanyam.
    let mut sigma = brenner_subrahmanyam_guess(market_price, f, t, config.iv_min, config.iv_max);

    // 5. Newton-Raphson loop.
    for i in 0..config.nr_max_iterations {
        let model_price = pricing::price(f, k, t, sigma, r, is_call);
        let v = pricing::vega(f, k, t, sigma, r);

        // Vega floor: if too small, NR step is enormous. Fall back to Brent.
        if v.abs() < config.vega_floor {
            break;
        }

        let diff = model_price - market_price;
        let step = diff / v;

        // Converge in volatility space: a small Newton step means σ is within
        // `iv_tolerance` of the root. Scale-free, and cannot report a flat
        // price region as converged (audit H-2).
        if step.abs() < config.iv_tolerance {
            return SolverResult {
                iv: sigma,
                method: SolverMethod::NewtonRaphson,
                iterations: i + 1,
                converged: true,
                status: SolverStatus::Converged,
                residual: diff.abs(),
            };
        }

        sigma -= step;
        sigma = sigma.clamp(config.iv_min, config.iv_max);
    }

    // 6. NR did not converge: fall back to Brent.
    brent_solve(market_price, f, k, t, r, is_call, config)
}

/// Solves bid, ask, and mid IV independently.
///
/// Any individual solve failure does not block the others.
///
/// Returns `(bid_result, ask_result, mid_result)`.
#[must_use]
#[allow(clippy::too_many_arguments)] // bid/ask/mid + 4 market inputs + is_call + config — natural arity
pub fn solve_iv_triple(
    bid_price: f64,
    ask_price: f64,
    mid_price: f64,
    f: f64,
    k: f64,
    t: f64,
    r: f64,
    is_call: bool,
    config: &SolverConfig,
) -> (SolverResult, SolverResult, SolverResult) {
    let bid_result = solve_iv(bid_price, f, k, t, r, is_call, config);
    let ask_result = solve_iv(ask_price, f, k, t, r, is_call, config);
    let mid_result = solve_iv(mid_price, f, k, t, r, is_call, config);
    (bid_result, ask_result, mid_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::{call_price, put_price};

    fn default_config() -> SolverConfig {
        SolverConfig::default()
    }

    #[test]
    fn atm_call_converges_nr() {
        let config = default_config();
        let market_price = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);
        assert!(result.converged);
        assert!((result.iv - 0.20).abs() < 1e-6);
        assert_eq!(result.method, SolverMethod::NewtonRaphson);
        assert!(result.iterations < 10);
    }

    #[test]
    fn atm_put_converges_nr() {
        let config = default_config();
        let market_price = put_price(100.0, 100.0, 1.0, 0.20, 0.0);
        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, false, &config);
        assert!(result.converged);
        assert!((result.iv - 0.20).abs() < 1e-6);
    }

    #[test]
    fn deep_otm_call_converges() {
        let config = default_config();
        let market_price = call_price(100.0, 200.0, 0.5, 0.80, 0.0);
        let result = solve_iv(market_price, 100.0, 200.0, 0.5, 0.0, true, &config);
        assert!(result.converged);
        assert!((result.iv - 0.80).abs() < 1e-4);
    }

    #[test]
    fn deep_itm_put_converges() {
        let config = default_config();
        let market_price = put_price(100.0, 50.0, 0.5, 0.30, 0.0);
        let result = solve_iv(market_price, 100.0, 50.0, 0.5, 0.0, false, &config);
        assert!(result.converged);
        assert!((result.iv - 0.30).abs() < 0.01);
    }

    #[test]
    fn near_expiry_returns_nan_with_status() {
        let config = default_config();
        // T = 0.0001 years ~ 0.876 hours < 2 hours cutoff
        let result = solve_iv(5.0, 105.0, 100.0, 0.0001, 0.0, true, &config);
        // Audit H-1: near-expiry is NOT a solved IV. It must not claim
        // `converged`; iv is NaN and the status says why.
        assert!(!result.converged);
        assert!(result.iv.is_nan());
        assert_eq!(result.status, SolverStatus::NearExpiryIntrinsic);
    }

    #[test]
    fn negative_time_value_flagged() {
        let config = default_config();
        // ITM call: intrinsic = 10, market_price = 9 < intrinsic
        let result = solve_iv(9.0, 110.0, 100.0, 1.0, 0.0, true, &config);
        assert!(!result.converged);
        assert!(result.iv.is_nan());
        assert_eq!(result.status, SolverStatus::BelowIntrinsic);
    }

    /// Regression: at non-zero rate, ITM call/put prices that lie between
    /// the discounted intrinsic `df·(F − K)` and the undiscounted intrinsic
    /// `F − K` are feasible Black-76 outputs. The solver must NOT reject
    /// them as "negative time value". Prior to the fix the gate compared
    /// against undiscounted intrinsic and rejected legitimate prices.
    #[test]
    fn itm_call_between_discounted_and_undiscounted_intrinsic_accepted() {
        let config = default_config();
        let f = 110.0_f64;
        let k = 100.0_f64;
        let t = 0.5_f64;
        let r = 0.10_f64;
        let sigma_true = 0.05_f64;
        let market = call_price(f, k, t, sigma_true, r);

        // Sanity: market sits in the bug zone.
        let intrinsic = f - k;
        let discounted = intrinsic * (-r * t).exp();
        assert!(
            market > discounted && market < intrinsic,
            "test inputs no longer hit the bug zone: market={market}, df·intr={discounted}, intr={intrinsic}",
        );

        let result = solve_iv(market, f, k, t, r, true, &config);
        assert!(
            result.converged,
            "feasible ITM call price must converge, got {result:?}",
        );
        assert!(
            (result.iv - sigma_true).abs() < 1e-4,
            "expected σ ≈ {sigma_true}, got {}",
            result.iv,
        );
    }

    /// Symmetric regression for ITM puts (K > F at non-zero rate).
    #[test]
    fn itm_put_between_discounted_and_undiscounted_intrinsic_accepted() {
        let config = default_config();
        let f = 100.0_f64;
        let k = 110.0_f64;
        let t = 0.5_f64;
        let r = 0.10_f64;
        let sigma_true = 0.05_f64;
        let market = put_price(f, k, t, sigma_true, r);

        let intrinsic = k - f;
        let discounted = intrinsic * (-r * t).exp();
        assert!(
            market > discounted && market < intrinsic,
            "test inputs no longer hit the bug zone: market={market}, df·intr={discounted}, intr={intrinsic}",
        );

        let result = solve_iv(market, f, k, t, r, false, &config);
        assert!(
            result.converged,
            "feasible ITM put price must converge, got {result:?}",
        );
        assert!(
            (result.iv - sigma_true).abs() < 1e-4,
            "expected σ ≈ {sigma_true}, got {}",
            result.iv,
        );
    }

    #[test]
    fn zero_market_price_returns_nan() {
        let config = default_config();
        let result = solve_iv(0.0, 100.0, 100.0, 1.0, 0.0, true, &config);
        assert!(!result.converged);
        assert!(result.iv.is_nan());
        assert_eq!(result.status, SolverStatus::NonPositivePrice);
    }

    /// Audit CRIT-A-01 regression: when the true IV lies above `iv_max`, the
    /// price is unattainable in `[iv_min, iv_max]`, so Brent finds no sign
    /// change. The solver must return `iv = NaN`, `converged = false`, and
    /// `status = NoBracketInRange` — never a clamped boundary marked converged.
    #[test]
    fn brent_no_bracket_returns_nan_iv() {
        // F=K=100, T=1, sigma=2.0 (200%): price ~= 79.07, but iv_max=1.0.
        let market_price = call_price(100.0, 100.0, 1.0, 2.0, 0.0);
        let config = SolverConfig::builder()
            .iv_min(0.01)
            .iv_max(1.0) // below the actual sigma=2.0
            .build();

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);
        assert!(result.iv.is_nan());
        assert!(!result.converged);
        assert_eq!(result.status, SolverStatus::NoBracketInRange);
    }

    #[test]
    fn iv_above_iv_max_returns_nan() {
        let mut config = default_config();
        config.iv_max = 2.0;
        let market_price = call_price(100.0, 100.0, 1.0, 3.0, 0.0);
        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);
        // Never a clamped boundary marked converged.
        assert!(result.iv.is_nan());
        assert!(!result.converged);
        assert_eq!(result.status, SolverStatus::NoBracketInRange);
    }

    /// Audit H-2: convergence is decided in σ-space, so the solver works at
    /// crypto scale (F ~ 100k) where an absolute `1e-8` price tolerance is
    /// ~`1e-13` relative and effectively unreachable.
    #[test]
    fn large_forward_atm_converges() {
        let config = default_config();
        let f = 100_000.0;
        let market = call_price(f, f, 0.25, 0.80, 0.0);
        let result = solve_iv(market, f, f, 0.25, 0.0, true, &config);
        assert!(result.converged, "large-F ATM should converge: {result:?}");
        assert_eq!(result.status, SolverStatus::Converged);
        assert!((result.iv - 0.80).abs() < 1e-4, "iv={}", result.iv);
    }

    /// Audit H-2: in a vega-weak region the old absolute-price gate could
    /// report `converged` at a σ far from the true root (any σ giving a price
    /// within `1e-8` of a near-zero market). σ-space convergence recovers the
    /// actual generating σ, or honestly reports a non-solution (NaN).
    #[test]
    fn deep_otm_recovers_true_sigma_not_flat_endpoint() {
        let config = default_config();
        let f = 100.0;
        let k = 250.0;
        let t = 0.10;
        let true_sigma = 0.60;
        let market = call_price(f, k, t, true_sigma, 0.0);
        let result = solve_iv(market, f, k, t, 0.0, true, &config);
        if result.converged {
            assert!(
                (result.iv - true_sigma).abs() < 1e-3,
                "converged to the wrong σ: got {}, true {true_sigma}",
                result.iv,
            );
        } else {
            assert!(result.iv.is_nan(), "non-converged result must be NaN");
        }
    }

    #[test]
    fn nonzero_risk_free_rate() {
        let config = default_config();
        let r = 0.05;
        let market_price = call_price(100.0, 100.0, 1.0, 0.25, r);
        let result = solve_iv(market_price, 100.0, 100.0, 1.0, r, true, &config);
        assert!(result.converged);
        assert!((result.iv - 0.25).abs() < 1e-6);
    }

    #[test]
    fn solve_iv_triple_independent() {
        let config = default_config();
        let bid = call_price(100.0, 100.0, 1.0, 0.18, 0.0);
        let ask = call_price(100.0, 100.0, 1.0, 0.22, 0.0);
        let mid = call_price(100.0, 100.0, 1.0, 0.20, 0.0);

        let (bid_r, ask_r, mid_r) =
            solve_iv_triple(bid, ask, mid, 100.0, 100.0, 1.0, 0.0, true, &config);

        assert!(bid_r.converged && ask_r.converged && mid_r.converged);
        assert!((bid_r.iv - 0.18).abs() < 1e-6);
        assert!((ask_r.iv - 0.22).abs() < 1e-6);
        assert!((mid_r.iv - 0.20).abs() < 1e-6);
    }

    #[test]
    fn solve_iv_triple_partial_failure() {
        let config = default_config();
        let ask = call_price(100.0, 100.0, 1.0, 0.22, 0.0);
        let mid = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
        let (bid_r, ask_r, mid_r) =
            solve_iv_triple(0.0, ask, mid, 100.0, 100.0, 1.0, 0.0, true, &config);
        assert!(!bid_r.converged);
        assert!(ask_r.converged);
        assert!(mid_r.converged);
    }
}
