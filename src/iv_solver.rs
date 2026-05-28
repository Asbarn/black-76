//! Implied volatility solver with Newton-Raphson + Brent fallback.
//!
//! Solves for `σ` such that `Black-76 price(σ) == market_price` within a
//! configurable tolerance.
//!
//! ## Convergence checking
//!
//! Callers **must** check [`SolverResult::converged`] before consuming `iv`.
//! When the market price is outside the feasible Black-76 range (below
//! intrinsic, above `F·exp(-rT)`, or otherwise unattainable for any
//! `σ ∈ [iv_min, iv_max]`), `iv` is set to [`f64::NAN`] and `converged` is
//! `false`. The `residual` field reports `|model_price − market_price|` at
//! the best endpoint examined.
//!
//! ## Edge cases handled
//!
//! - **Near-expiry**: `t < near_expiry_cutoff_hours` returns
//!   `iv = 0.0, converged = true, method = NewtonRaphson, iterations = 0`
//!   (intrinsic-pricing sentinel).
//! - **Zero/negative price**: returns `iv = iv_min, converged = false`.
//! - **Negative time value** (`market_price < intrinsic`): returns
//!   `iv = iv_min, converged = false`.
//! - **Near-zero vega** (deep OTM/ITM): NR falls back to Brent for
//!   guaranteed convergence inside the bracket.
//! - **No root in bracket**: returns `iv = f64::NAN, converged = false`.

use crate::config::SolverConfig;
use crate::pricing;
use crate::types::{SolverMethod, SolverResult};

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
        return (iv_min + iv_max) / 2.0;
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
        // Converged within tolerance.
        if fb.abs() < config.price_tolerance {
            return SolverResult {
                iv: b,
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged: true,
                residual: fb.abs(),
            };
        }

        // Bracket collapsed to machine precision.
        if (b - a).abs() < f64::EPSILON * (a.abs() + b.abs()).max(1.0) {
            return SolverResult {
                iv: b,
                method: SolverMethod::Brent,
                iterations: i + 1,
                converged: fb.abs() < config.price_tolerance,
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
        let midpoint = (a + b) / 2.0;
        let reject_interpolation = {
            let bound1 = (3.0 * a + b) / 4.0;
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

    // Hit max iterations without converging.
    SolverResult {
        iv: b,
        method: SolverMethod::Brent,
        iterations: config.brent_max_iterations,
        converged: fb.abs() < config.price_tolerance,
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
            iv: 0.0,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: true,
            residual: 0.0,
        };
    }

    // 2. Zero or negative market price: no valid IV exists.
    if market_price <= 0.0 {
        return SolverResult {
            iv: config.iv_min,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            residual: market_price.abs(),
        };
    }

    // 3. Negative time value: market_price < intrinsic.
    let intrinsic = pricing::intrinsic_value(f, k, is_call);
    if market_price < intrinsic {
        return SolverResult {
            iv: config.iv_min,
            method: SolverMethod::NewtonRaphson,
            iterations: 0,
            converged: false,
            residual: (intrinsic - market_price).abs(),
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

        if diff.abs() < config.price_tolerance {
            return SolverResult {
                iv: sigma,
                method: SolverMethod::NewtonRaphson,
                iterations: i + 1,
                converged: true,
                residual: diff.abs(),
            };
        }

        sigma -= diff / v;
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
    fn near_expiry_returns_intrinsic() {
        let config = default_config();
        // T = 0.0001 years ~ 0.876 hours < 2 hours cutoff
        let result = solve_iv(5.0, 105.0, 100.0, 0.0001, 0.0, true, &config);
        assert!(result.converged);
        assert!(result.iv.abs() < f64::EPSILON);
    }

    #[test]
    fn negative_time_value_flagged() {
        let config = default_config();
        // ITM call: intrinsic = 10, market_price = 9 < intrinsic
        let result = solve_iv(9.0, 110.0, 100.0, 1.0, 0.0, true, &config);
        assert!(!result.converged);
        assert!((result.iv - config.iv_min).abs() < f64::EPSILON);
    }

    #[test]
    fn zero_market_price_returns_iv_min() {
        let config = default_config();
        let result = solve_iv(0.0, 100.0, 100.0, 1.0, 0.0, true, &config);
        assert!(!result.converged);
        assert!((result.iv - config.iv_min).abs() < f64::EPSILON);
    }

    /// Audit CRIT-A-01 regression: when the price is below intrinsic by a
    /// large amount, the solver should NOT return iv_min/iv_max marked as
    /// converged. We pass through the negative-time-value gate (which already
    /// returns iv=iv_min, converged=false). The Brent fallback no-bracket
    /// path is exercised by the test below.
    #[test]
    fn brent_no_bracket_returns_nan_iv() {
        // Construct a price that yields fa·fb > 0 in Brent's bracket check.
        // For F=K=100, T=1, r=0, the call price is bounded in [intrinsic, F·df]
        // = [0, 100]. We've already gated negative-time-value (price < intrinsic).
        // To trigger Brent's no-bracket path we need the NR loop to exit on
        // vega-floor with sigma not converged, AND Brent to find fa·fb > 0.
        //
        // Test approach: construct a tight iv_max that's below the actual IV.
        // For F=100, K=100, T=1, sigma=2.0 (200%): price ~= 79.07
        let market_price = call_price(100.0, 100.0, 1.0, 2.0, 0.0);
        let config = SolverConfig::builder()
            .iv_min(0.01)
            .iv_max(1.0) // below the actual sigma=2.0
            .build();

        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);
        // Either Brent returns NaN (truly no bracket) or it returns iv=iv_max
        // with converged=false. Both are acceptable; what we MUST NOT see is
        // converged=true with a wrong iv.
        if !result.iv.is_nan() {
            // Then it should be clamped to iv_max and explicitly non-converged.
            assert!(
                !result.converged || (result.iv - 2.0).abs() < 0.01,
                "got iv={}, converged={}",
                result.iv,
                result.converged
            );
        } else {
            assert!(!result.converged);
        }
    }

    #[test]
    fn iv_clamped_at_upper_bound() {
        let mut config = default_config();
        config.iv_max = 2.0;
        let market_price = call_price(100.0, 100.0, 1.0, 3.0, 0.0);
        let result = solve_iv(market_price, 100.0, 100.0, 1.0, 0.0, true, &config);
        // Either NaN (no bracket) or clamped, but never falsely converged with
        // an iv > iv_max.
        if !result.iv.is_nan() {
            assert!(result.iv <= config.iv_max + f64::EPSILON);
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
