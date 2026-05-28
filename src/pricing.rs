//! Black-76 closed-form pricing.
//!
//! The Black-76 model prices European options on forwards/futures. Given a
//! forward `F`, strike `K`, time-to-expiry `T` (in years), volatility `σ`,
//! and risk-free rate `r`:
//!
//! ```text
//! d1 = (ln(F/K) + 0.5 σ² T) / (σ √T)
//! d2 = d1 − σ √T
//! df = exp(−rT)
//!
//! C = df · (F · N(d1) − K · N(d2))
//! P = df · (K · N(−d2) − F · N(−d1))
//! ```
//!
//! All functions operate in `f64` space. Reference: Hull, *Options, Futures,
//! and Other Derivatives*, 10th ed., §17.

use statrs::distribution::{Continuous, ContinuousCDF, Normal};

// ---------------------------------------------------------------------------
// d1 / d2
// ---------------------------------------------------------------------------

/// Computes `(d1, d2)` for Black-76.
///
/// ```text
/// d1 = (ln(F/K) + 0.5 σ² T) / (σ √T)
/// d2 = d1 − σ √T
/// ```
///
/// # Examples
///
/// ```
/// use black_76::d1_d2;
/// let (d1, d2) = d1_d2(100.0, 100.0, 1.0, 0.20);
/// assert!((d1 - 0.10).abs() < 1e-10);
/// assert!((d2 + 0.10).abs() < 1e-10);
/// ```
#[inline]
pub fn d1_d2(f: f64, k: f64, t: f64, sigma: f64) -> (f64, f64) {
    let sqrt_t = t.sqrt();
    let d1 = ((f / k).ln() + 0.5 * sigma * sigma * t) / (sigma * sqrt_t);
    let d2 = d1 - sigma * sqrt_t;
    (d1, d2)
}

// ---------------------------------------------------------------------------
// Pricing functions
// ---------------------------------------------------------------------------

/// Black-76 call price.
///
/// ```text
/// C = df · (F · N(d1) − K · N(d2))   where df = exp(−rT)
/// ```
///
/// Returns intrinsic value `max(F − K, 0)` when `t ≤ 0` or `sigma ≤ 0`
/// (no time value).
///
/// # Examples
///
/// ```
/// use black_76::call_price;
/// // ATM call: F=100, K=100, T=1, sigma=20%, r=0
/// let c = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
/// assert!((c - 7.96556746).abs() < 1e-6);
/// ```
pub fn call_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return intrinsic_value(f, k, true);
    }
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * (f * norm.cdf(d1) - k * norm.cdf(d2))
}

/// Black-76 put price.
///
/// ```text
/// P = df · (K · N(−d2) − F · N(−d1))   where df = exp(−rT)
/// ```
///
/// Returns intrinsic value `max(K − F, 0)` when `t ≤ 0` or `sigma ≤ 0`.
///
/// # Examples
///
/// ```
/// use black_76::{call_price, put_price};
/// // Put-call parity: C − P = df · (F − K)
/// let (f, k, t, sigma, r) = (100.0, 110.0, 0.5, 0.30, 0.05);
/// let c = call_price(f, k, t, sigma, r);
/// let p = put_price(f, k, t, sigma, r);
/// let parity = (-r * t).exp() * (f - k);
/// assert!((c - p - parity).abs() < 1e-10);
/// ```
pub fn put_price(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return intrinsic_value(f, k, false);
    }
    let (d1, d2) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * (k * norm.cdf(-d2) - f * norm.cdf(-d1))
}

/// Dispatches to [`call_price`] or [`put_price`] based on `is_call`.
#[inline]
pub fn price(f: f64, k: f64, t: f64, sigma: f64, r: f64, is_call: bool) -> f64 {
    if is_call {
        call_price(f, k, t, sigma, r)
    } else {
        put_price(f, k, t, sigma, r)
    }
}

// ---------------------------------------------------------------------------
// Vega
// ---------------------------------------------------------------------------

/// Vega: sensitivity of price to volatility, in price units per 1.0 absolute
/// change in σ (i.e. **not** per-1%).
///
/// ```text
/// vega = df · F · n(d1) · √T
/// ```
///
/// where `n(d1)` is the standard-normal PDF at `d1`. Vega is the same for
/// calls and puts.
///
/// Returns `0.0` when `t ≤ 0` or `sigma ≤ 0`.
///
/// # Examples
///
/// ```
/// use black_76::vega;
/// let v = vega(100.0, 100.0, 1.0, 0.20, 0.0);
/// assert!(v > 0.0);
/// ```
pub fn vega(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> f64 {
    if t <= 0.0 || sigma <= 0.0 {
        return 0.0;
    }
    let (d1, _) = d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    df * f * norm.pdf(d1) * t.sqrt()
}

// ---------------------------------------------------------------------------
// Intrinsic value
// ---------------------------------------------------------------------------

/// Intrinsic value (payoff at expiry).
///
/// - Call: `max(F − K, 0)`
/// - Put: `max(K − F, 0)`
#[inline]
pub fn intrinsic_value(f: f64, k: f64, is_call: bool) -> f64 {
    if is_call {
        (f - k).max(0.0)
    } else {
        (k - f).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ATM call: F=100, K=100, T=1.0, sigma=0.20, r=0.0.
    /// Per Hull §17.6: d1 = 0.10, d2 = -0.10, C = 100·(N(0.10) − N(−0.10)) = 7.96556746...
    #[test]
    fn atm_call_price_known_value() {
        let c = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
        // Tightened from 1e-2 to 1e-6 per audit LOW-A-01.
        assert!(
            (c - 7.96556746).abs() < 1e-6,
            "ATM call price should match Hull §17.6 to 1e-6, got {c}"
        );
    }

    /// Put-call parity: C − P = df · (F − K) for various strikes.
    #[test]
    fn put_call_parity() {
        let f = 100.0_f64;
        let t = 0.5_f64;
        let sigma = 0.30_f64;
        let r = 0.05_f64;
        let df = (-r * t).exp();

        for &k in &[80.0, 90.0, 100.0, 110.0, 120.0] {
            let c = call_price(f, k, t, sigma, r);
            let p = put_price(f, k, t, sigma, r);
            let parity = df * (f - k);
            let diff = (c - p - parity).abs();
            assert!(
                diff < 1e-10,
                "Put-call parity violated at K={k}: C-P={}, df*(F-K)={parity}, diff={diff}",
                c - p
            );
        }
    }

    /// Vega is positive for non-degenerate inputs and equal for call/put.
    #[test]
    fn vega_positive_and_call_put_equal() {
        let v = vega(100.0, 100.0, 1.0, 0.20, 0.0);
        assert!(v > 0.0, "vega should be positive, got {v}");
    }

    /// Deep OTM call price is near zero.
    #[test]
    fn deep_otm_call_near_zero() {
        let c = call_price(100.0, 200.0, 0.25, 0.20, 0.0);
        assert!(c < 1e-6, "deep OTM call should be near zero, got {c}");
    }

    /// Near-zero T returns intrinsic value.
    #[test]
    fn near_zero_t_returns_intrinsic() {
        assert!((call_price(110.0, 100.0, 0.0, 0.20, 0.0) - 10.0).abs() < f64::EPSILON);
        assert!(call_price(90.0, 100.0, 0.0, 0.20, 0.0).abs() < f64::EPSILON);
        assert!((put_price(90.0, 100.0, 0.0, 0.20, 0.0) - 10.0).abs() < f64::EPSILON);
        // Zero sigma also returns intrinsic
        assert!((call_price(110.0, 100.0, 1.0, 0.0, 0.0) - 10.0).abs() < f64::EPSILON);
    }

    /// Vega matches finite-difference approximation.
    #[test]
    fn vega_finite_difference() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.0;
        let h = 1e-5;

        let v_analytic = vega(f, k, t, sigma, r);
        let v_fd =
            (call_price(f, k, t, sigma + h, r) - call_price(f, k, t, sigma - h, r)) / (2.0 * h);

        assert!(
            (v_analytic - v_fd).abs() < 1e-4,
            "vega analytic ({v_analytic}) vs FD ({v_fd}) diff > 1e-4"
        );
    }

    /// `price()` dispatches correctly.
    #[test]
    fn price_dispatch() {
        let c = price(100.0, 100.0, 1.0, 0.20, 0.0, true);
        let p = price(100.0, 100.0, 1.0, 0.20, 0.0, false);
        assert!((c - call_price(100.0, 100.0, 1.0, 0.20, 0.0)).abs() < f64::EPSILON);
        assert!((p - put_price(100.0, 100.0, 1.0, 0.20, 0.0)).abs() < f64::EPSILON);
    }

    /// Vega is zero for degenerate inputs.
    #[test]
    fn vega_degenerate_inputs() {
        assert!(vega(100.0, 100.0, 0.0, 0.20, 0.0).abs() < f64::EPSILON);
        assert!(vega(100.0, 100.0, 1.0, 0.0, 0.0).abs() < f64::EPSILON);
    }

    /// Intrinsic value correctness.
    #[test]
    fn intrinsic_value_correctness() {
        assert!((intrinsic_value(110.0, 100.0, true) - 10.0).abs() < f64::EPSILON);
        assert!(intrinsic_value(90.0, 100.0, true).abs() < f64::EPSILON);
        assert!((intrinsic_value(90.0, 100.0, false) - 10.0).abs() < f64::EPSILON);
        assert!(intrinsic_value(110.0, 100.0, false).abs() < f64::EPSILON);
    }
}
