//! First-order Greeks for Black-76 options.
//!
//! Computes delta, gamma, vega (per 1% IV move), theta (per day), and rho
//! (per 1% rate move) for a single option.
//!
//! Reference: Hull, *Options, Futures, and Other Derivatives*, 10th ed., §17.8.
//! Haug, *The Complete Guide to Option Pricing Formulas*, 2nd ed., §1.1.5.

use statrs::distribution::{Continuous, ContinuousCDF, Normal};

use crate::pricing;
use crate::types::InstrumentGreeks;

/// Days per year for theta normalization.
const DAYS_PER_YEAR: f64 = 365.25;

/// Computes delta, gamma, vega (per-1% IV move), theta (per-day), and rho
/// (per-1% rate move) for a single Black-76 option.
///
/// # Parameters
/// - `f`: forward / futures price (must be `> 0`)
/// - `k`: strike (must be `> 0`)
/// - `t`: time to expiry in years
/// - `sigma`: implied volatility (annualized)
/// - `r`: risk-free rate
/// - `is_call`: `true` for call, `false` for put
///
/// # Edge cases
///
/// When `t <= 0` **or** `sigma <= 0` (no time value), returns the intrinsic
/// delta (`1.0` for an ITM call, `-1.0` for an ITM put, `0.0` for OTM; the
/// at-the-money `F == K` boundary maps to `0.0`), with
/// `gamma = vega = theta = rho = 0`. This mirrors `pricing`'s `sigma <= 0`
/// guard so the Greeks never return `NaN` for a degenerate but finite input.
///
/// # Sign and unit conventions
///
/// - **Delta**: dimensionless; positive for calls, negative for puts.
/// - **Gamma**: `d^2 price / dF^2`, per unit forward (raw); identical for
///   calls and puts.
/// - **Vega**: per **1%** absolute change in IV (raw `dC/dsigma / 100`), the
///   trader convention; identical for calls and puts.
/// - **Theta**: per calendar day (year = 365.25 days); negative for long
///   positions (time decay).
/// - **Rho**: per **1%** absolute change in the rate (`dprice/dr / 100`);
///   negative under Black-76 (`dC/dr = -T*price`).
///
/// # Examples
///
/// ```
/// use black_76::compute_greeks;
/// let g = compute_greeks(100.0, 100.0, 1.0, 0.20, 0.0, true);
/// assert!(g.delta > 0.5 && g.delta < 0.6); // ATM call ~0.54
/// assert!(g.gamma > 0.0);
/// assert!(g.vega > 0.0);
/// assert!(g.theta < 0.0);                  // long options decay
/// assert!(g.rho < 0.0);                    // Black-76 rho = -T*price
/// ```
#[must_use]
pub fn compute_greeks(
    f: f64,
    k: f64,
    t: f64,
    sigma: f64,
    r: f64,
    is_call: bool,
) -> InstrumentGreeks {
    // Degenerate case: expired (t <= 0) or zero/negative volatility. Both have
    // no time value and no smooth sensitivities; return the intrinsic delta.
    // Without the `sigma <= 0` arm, `d1 = 0/0 = NaN` at the money, so
    // delta/theta would be NaN, unlike `pricing`, which already guards
    // `sigma <= 0`. This keeps the two modules consistent and NaN-free.
    if t <= 0.0 || sigma <= 0.0 {
        let delta = if is_call {
            if f > k { 1.0 } else { 0.0 }
        } else if k > f {
            -1.0
        } else {
            0.0
        };
        return InstrumentGreeks {
            delta,
            gamma: 0.0,
            vega: 0.0,
            theta: 0.0,
            rho: 0.0,
        };
    }

    let (d1, d2) = pricing::d1_d2(f, k, t, sigma);
    let norm = Normal::standard();
    let df = (-r * t).exp();
    let n_d1 = norm.pdf(d1);
    let sqrt_t = t.sqrt();

    // Delta.
    let delta = if is_call {
        df * norm.cdf(d1)
    } else {
        df * (norm.cdf(d1) - 1.0)
    };

    // Gamma: d^2 price / dF^2 = df * n(d1) / (F * sigma * sqrt(T)); same call/put.
    let gamma = df * n_d1 / (f * sigma * sqrt_t);

    // Vega: per-1% IV move (trader convention).
    let vega = pricing::vega(f, k, t, sigma, r) / 100.0;

    // Black-76 price of this option (reused by theta's carry term and rho).
    let price = if is_call {
        df * f.mul_add(norm.cdf(d1), -(k * norm.cdf(d2)))
    } else {
        df * k.mul_add(norm.cdf(-d2), -(f * norm.cdf(-d1)))
    };

    // Theta (per day). Black-76 theta per Hull §17.8 eq 17.4 and Haug §1.1.5:
    //
    //   theta = -df * F * n(d1) * sigma / (2*sqrt(T)) + r * price
    //
    // Derivation: as wall-clock time advances by dt, time-to-expiry T
    // decreases by dt, so dC/dt = -dC/dT. Computing dC/dT gives
    // -r*C + F*df*n(d1)*sigma/(2*sqrt(T)) (using F*n(d1) = K*n(d2)).
    // Therefore theta = -dC/dT = r*price - F*df*n(d1)*sigma/(2*sqrt(T)).
    let time_decay = -df * f * n_d1 * sigma / (2.0 * sqrt_t);
    let carry_cost = r * price; // +r*C (call) / +r*P (put)
    let theta = (time_decay + carry_cost) / DAYS_PER_YEAR;

    // Rho: dprice/dr. In Black-76 the forward F is held fixed as r varies, so
    // dC/dr = -T*C and dP/dr = -T*P. Reported per-1% (/100), like vega.
    let rho = (-t * price) / 100.0;

    InstrumentGreeks {
        delta,
        gamma,
        vega,
        theta,
        rho,
    }
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

    /// Theta carry term has the correct sign and magnitude, checked against a
    /// central finite difference at r=0.05 (a wrong carry sign produces a ~10%
    /// relative error here).
    #[test]
    fn theta_sign_correct_at_nonzero_rate() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.05;

        let g = compute_greeks(f, k, t, sigma, r, true);

        // theta (per year) = -dC/dT via a symmetric step in T, then per-day.
        let h = 1e-4;
        let c_up = call_price(f, k, t + h, sigma, r);
        let c_dn = call_price(f, k, t - h, sigma, r);
        let theta_fd_per_day = -(c_up - c_dn) / (2.0 * h) / DAYS_PER_YEAR;

        let rel_err = (g.theta - theta_fd_per_day).abs() / theta_fd_per_day.abs().max(1e-6);
        assert!(
            rel_err < 1e-5,
            "theta analytic {} vs FD {} rel err {}",
            g.theta,
            theta_fd_per_day,
            rel_err
        );
    }

    /// Same central-difference check for puts.
    #[test]
    fn theta_sign_correct_at_nonzero_rate_put() {
        let f = 100.0;
        let k = 100.0;
        let t = 1.0;
        let sigma = 0.20;
        let r = 0.05;

        let g = compute_greeks(f, k, t, sigma, r, false);
        let h = 1e-4;
        let p_up = put_price(f, k, t + h, sigma, r);
        let p_dn = put_price(f, k, t - h, sigma, r);
        let theta_fd_per_day = -(p_up - p_dn) / (2.0 * h) / DAYS_PER_YEAR;

        let rel_err = (g.theta - theta_fd_per_day).abs() / theta_fd_per_day.abs().max(1e-6);
        assert!(
            rel_err < 1e-5,
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

    #[test]
    fn gamma_positive_and_matches_fd() {
        let (f, k, t, sigma, r) = (100.0, 100.0, 1.0, 0.20, 0.0);
        let g = compute_greeks(f, k, t, sigma, r, true);
        assert!(g.gamma > 0.0);

        // Gamma via central second difference of price in F.
        let h = 1e-2;
        let p_up = call_price(f + h, k, t, sigma, r);
        let p_dn = call_price(f - h, k, t, sigma, r);
        let p_0 = call_price(f, k, t, sigma, r);
        let fd_gamma = (p_up - 2.0 * p_0 + p_dn) / (h * h);
        assert!(
            (g.gamma - fd_gamma).abs() < 1e-4,
            "gamma analytic {} vs FD {}",
            g.gamma,
            fd_gamma
        );

        // Gamma is identical for calls and puts.
        let gp = compute_greeks(f, k, t, sigma, r, false);
        assert!((g.gamma - gp.gamma).abs() < 1e-12);
    }

    #[test]
    fn rho_matches_fd_and_sign() {
        let (f, k, t, sigma, r) = (100.0, 100.0, 1.0, 0.20, 0.05);
        let g = compute_greeks(f, k, t, sigma, r, true);
        assert!(g.rho < 0.0, "Black-76 call rho = -T*C < 0");

        // FD of price wrt r, then per-1%.
        let h = 1e-6;
        let c_up = call_price(f, k, t, sigma, r + h);
        let c_dn = call_price(f, k, t, sigma, r - h);
        let fd_rho = ((c_up - c_dn) / (2.0 * h)) / 100.0;
        assert!(
            (g.rho - fd_rho).abs() < 1e-6,
            "rho analytic {} vs FD {}",
            g.rho,
            fd_rho
        );

        // Put rho = -T*P.
        let gp = compute_greeks(f, k, t, sigma, r, false);
        let p = put_price(f, k, t, sigma, r);
        assert!((gp.rho - (-t * p) / 100.0).abs() < 1e-12);
    }

    /// `compute_greeks` must not return `NaN` at `sigma = 0` (ATM is the
    /// dangerous case: `d1 = 0/0`). This is reachable via the near-expiry path
    /// of the IV solver, which yields an intrinsic (zero-vol) regime.
    #[test]
    fn greeks_finite_at_zero_sigma() {
        for &is_call in &[true, false] {
            let g = compute_greeks(100.0, 100.0, 1.0, 0.0, 0.05, is_call);
            assert!(g.delta.is_finite(), "delta NaN at sigma=0");
            assert!(g.gamma.is_finite(), "gamma NaN at sigma=0");
            assert!(g.vega.is_finite(), "vega NaN at sigma=0");
            assert!(g.theta.is_finite(), "theta NaN at sigma=0");
            assert!(g.rho.is_finite(), "rho NaN at sigma=0");
        }
        // Off-the-money zero-sigma is finite too.
        let g = compute_greeks(120.0, 100.0, 1.0, 0.0, 0.0, true);
        assert!(g.delta.is_finite() && g.theta.is_finite());
    }
}
