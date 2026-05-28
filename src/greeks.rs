//! First-order Greeks for Black-76 options.
//!
//! Computes delta, vega (per 1% IV move), and theta (per day) for a single
//! option. Gamma is intentionally omitted; if needed, compute via finite
//! differences against [`pricing::price`].
//!
//! Reference: Hull, *Options, Futures, and Other Derivatives*, 10th ed., §17.8.
//! Haug, *The Complete Guide to Option Pricing Formulas*, 2nd ed., §1.1.5.

use statrs::distribution::{Continuous, ContinuousCDF, Normal};

use crate::pricing;
use crate::types::InstrumentGreeks;

/// Days per year for theta normalization.
const DAYS_PER_YEAR: f64 = 365.25;

/// Computes delta, vega (per-1% IV move), and theta (per-day) for a single
/// Black-76 option.
///
/// # Parameters
/// - `f`: forward / futures price
/// - `k`: strike
/// - `t`: time to expiry in years
/// - `sigma`: implied volatility (annualized)
/// - `r`: risk-free rate
/// - `is_call`: `true` for call, `false` for put
///
/// # Edge cases
///
/// When `t <= 0`, returns intrinsic delta (`1.0` for ITM call, `-1.0` for
/// ITM put, `0.0` for OTM), with `vega = 0` and `theta = 0`.
///
/// # Sign and unit conventions
///
/// - **Vega** is reported per-1% absolute change in IV (i.e., the raw
///   `dC/dσ` is divided by 100). This is the trader convention.
/// - **Theta** is reported per-day, with year = 365.25 days. Negative for
///   long positions (time decay).
///
/// # Examples
///
/// ```
/// use black_76::compute_greeks;
/// let g = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
/// assert!(g.delta > 0.5 && g.delta < 0.6); // ATM call ~0.54
/// assert!(g.vega > 0.0);
/// assert!(g.theta < 0.0);                  // long options decay
/// ```
pub fn compute_greeks(
    f: f64,
    k: f64,
    t: f64,
    sigma: f64,
    r: f64,
    is_call: bool,
) -> InstrumentGreeks {
    // Degenerate case: expired.
    if t <= 0.0 {
        let delta = if is_call {
            if f > k { 1.0 } else { 0.0 }
        } else if k > f {
            -1.0
        } else {
            0.0
        };
        return InstrumentGreeks {
            delta,
            vega: 0.0,
            theta: 0.0,
        };
    }

    let (d1, d2) = pricing::d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();

    // Delta.
    let delta = if is_call {
        df * norm.cdf(d1)
    } else {
        df * (norm.cdf(d1) - 1.0)
    };

    // Vega: per-1% IV move (trader convention).
    let raw_vega = pricing::vega(f, k, t, sigma, r);
    let vega = raw_vega / 100.0;

    // Theta (per day).
    //
    // Per audit HIGH-A-01: the prediction repo's original code computed
    // carry_cost = -r*C (wrong sign). Correct Black-76 theta per Hull §17.8
    // eq 17.4 and Haug §1.1.5:
    //
    //   theta_call = -df * F * n(d1) * sigma / (2*sqrt(T)) + r * C
    //   theta_put  = -df * F * n(d1) * sigma / (2*sqrt(T)) + r * P
    //
    // Derivation: as wall-clock time advances by dt, time-to-expiry T
    // decreases by dt. So dC/dt = -dC/dT. Computing dC/dT we get
    // -r*C + F*df*n(d1)*sigma/(2*sqrt(T)) (using F*n(d1) = K*n(d2)).
    // Therefore theta = -dC/dT = r*C - F*df*n(d1)*sigma/(2*sqrt(T)).
    let sqrt_t = t.sqrt();
    let n_d1 = norm.pdf(d1);

    let time_decay = -df * f * n_d1 * sigma / (2.0 * sqrt_t);

    let carry_cost = if is_call {
        r * df * (f * norm.cdf(d1) - k * norm.cdf(d2)) // +r * C
    } else {
        r * df * (k * norm.cdf(-d2) - f * norm.cdf(-d1)) // +r * P
    };

    let theta_per_year = time_decay + carry_cost;
    let theta = theta_per_year / DAYS_PER_YEAR;

    InstrumentGreeks { delta, vega, theta }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::{call_price, put_price};

    #[test]
    fn atm_call_delta_near_half() {
        let g = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(
            (g.delta - 0.5).abs() < 0.05,
            "ATM call delta ~0.5, got {}",
            g.delta
        );
    }

    #[test]
    fn deep_itm_call_delta_near_one() {
        let g = compute_greeks(150.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(g.delta > 0.95);
    }

    #[test]
    fn vega_positive_and_call_put_equal() {
        let gc = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        let gp = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, false);
        assert!(gc.vega > 0.0);
        assert!(gp.vega > 0.0);
        assert!((gc.vega - gp.vega).abs() < 1e-10);
    }

    #[test]
    fn otm_put_delta_negative() {
        let g = compute_greeks(100.0, 90.0, 1.0, 0.20, 0.0, false);
        assert!(g.delta < 0.0);
    }

    #[test]
    fn theta_negative() {
        let g = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        assert!(g.theta < 0.0);
    }

    #[test]
    fn expired_returns_intrinsic_delta() {
        let g = compute_greeks(110.0, 100.0, 0.0, 0.20, 0.0, true);
        assert!((g.delta - 1.0).abs() < f64::EPSILON);
        assert!(g.vega.abs() < f64::EPSILON);
        assert!(g.theta.abs() < f64::EPSILON);

        let g = compute_greeks(90.0, 100.0, 0.0, 0.20, 0.0, false);
        assert!((g.delta + 1.0).abs() < f64::EPSILON);
    }

    /// Audit HIGH-A-01 regression: theta carry term has the correct sign.
    /// Verified via finite-difference at r=0.05 (the case where the old wrong
    /// sign would produce a ~10% relative error).
    #[test]
    fn theta_sign_correct_at_nonzero_rate() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.05;

        let g = compute_greeks(f, k, t, sigma, r, true);

        // Finite-difference theta as benchmark: dC/dt where wall-clock time
        // advances; we let T decrease by dt.
        let dt = 1.0 / DAYS_PER_YEAR; // one day
        let c_now = call_price(f, k, t, sigma, r);
        let c_later = call_price(f, k, t - dt, sigma, r);
        let theta_fd_per_day = c_later - c_now;

        let rel_err = (g.theta - theta_fd_per_day).abs() / theta_fd_per_day.abs().max(1e-6);
        assert!(
            rel_err < 1e-3,
            "theta analytic {} vs FD {} rel err {}; \
             if this fails the theta carry-term sign is wrong",
            g.theta,
            theta_fd_per_day,
            rel_err
        );
    }

    /// Same FD check for puts.
    #[test]
    fn theta_sign_correct_at_nonzero_rate_put() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.05;

        let g = compute_greeks(f, k, t, sigma, r, false);
        let dt = 1.0 / DAYS_PER_YEAR;
        let p_now = put_price(f, k, t, sigma, r);
        let p_later = put_price(f, k, t - dt, sigma, r);
        let theta_fd_per_day = p_later - p_now;

        let rel_err = (g.theta - theta_fd_per_day).abs() / theta_fd_per_day.abs().max(1e-6);
        assert!(
            rel_err < 1e-3,
            "put theta analytic vs FD rel err {}",
            rel_err
        );
    }

    #[test]
    fn nonzero_rate_affects_delta() {
        let g0 = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
        let g5 = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.05, true);
        assert!(g5.delta < g0.delta);
    }
}
