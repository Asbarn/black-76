//! Ergonomic, typo-resistant input structs.
//!
//! The free functions in [`pricing`](crate::pricing), [`greeks`](crate::greeks),
//! and [`iv_solver`](crate::iv_solver) take several positional `f64` arguments,
//! which is easy to mis-order (`f`/`k`, `t`/`r`). [`BlackInputs`] and
//! [`IvQuery`] name every field, so call sites are self-documenting and an
//! argument swap becomes a field name rather than a silent wrong answer. The
//! free functions remain available and are not deprecated — this is a thin,
//! zero-cost convenience layer over them.

use crate::config::SolverConfig;
use crate::greeks::compute_greeks;
use crate::iv_solver::solve_iv;
use crate::pricing::{call_price, d1_d2, intrinsic_value, price, put_price, vega};
use crate::types::{InstrumentGreeks, SolverResult};

/// Named inputs for Black-76 pricing and Greeks: forward `f`, strike `k`,
/// time-to-expiry `t` (in years), volatility `sigma`, and rate `r`.
///
/// A typo-resistant alternative to the positional free functions — every field
/// is named, so `f`/`k` and `t`/`r` cannot be silently swapped.
///
/// ```
/// use black_76::{BlackInputs, call_price};
/// let inputs = BlackInputs::new(100.0, 100.0, 1.0, 0.20, 0.0);
/// let c = inputs.call_price();
/// assert!((c - call_price(100.0, 100.0, 1.0, 0.20, 0.0)).abs() < 1e-12);
/// let g = inputs.greeks(true);
/// assert!(g.delta > 0.0 && g.gamma > 0.0);
/// ```
///
/// New fields may be added in future minor versions; construct via
/// [`new`](Self::new) (the type is `#[non_exhaustive]`, so struct-literal
/// construction is unavailable to downstream crates). The public fields
/// remain readable and serde-serializable.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct BlackInputs {
    /// Forward / futures price.
    pub f: f64,
    /// Strike.
    pub k: f64,
    /// Time to expiry, in years.
    pub t: f64,
    /// Volatility (annualized).
    pub sigma: f64,
    /// Risk-free rate (annualized, continuously compounded).
    pub r: f64,
}

impl BlackInputs {
    /// Construct inputs from forward, strike, time (years), volatility, rate.
    #[must_use]
    pub const fn new(f: f64, k: f64, t: f64, sigma: f64, r: f64) -> Self {
        Self { f, k, t, sigma, r }
    }

    /// `(d1, d2)` for these inputs (independent of `r`).
    #[must_use]
    pub fn d1_d2(&self) -> (f64, f64) {
        d1_d2(self.f, self.k, self.t, self.sigma)
    }

    /// Black-76 call price.
    #[must_use]
    pub fn call_price(&self) -> f64 {
        call_price(self.f, self.k, self.t, self.sigma, self.r)
    }

    /// Black-76 put price.
    #[must_use]
    pub fn put_price(&self) -> f64 {
        put_price(self.f, self.k, self.t, self.sigma, self.r)
    }

    /// Call or put price, selected by `is_call`.
    #[must_use]
    pub fn price(&self, is_call: bool) -> f64 {
        price(self.f, self.k, self.t, self.sigma, self.r, is_call)
    }

    /// Vega (price change per 1.0 absolute change in volatility).
    #[must_use]
    pub fn vega(&self) -> f64 {
        vega(self.f, self.k, self.t, self.sigma, self.r)
    }

    /// First-order Greeks (delta, gamma, vega-per-1%, theta-per-day, rho-per-1%).
    #[must_use]
    pub fn greeks(&self, is_call: bool) -> InstrumentGreeks {
        compute_greeks(self.f, self.k, self.t, self.sigma, self.r, is_call)
    }

    /// Intrinsic value: `max(F − K, 0)` (call) or `max(K − F, 0)` (put).
    #[must_use]
    pub fn intrinsic_value(&self, is_call: bool) -> f64 {
        intrinsic_value(self.f, self.k, is_call)
    }
}

/// A market quote to invert for implied volatility: the observed
/// `market_price` plus the option's `f`, `k`, `t`, `r`, and `is_call`.
///
/// ```
/// use black_76::{IvQuery, SolverConfig, call_price};
/// let market = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
/// let result = IvQuery::new(market, 100.0, 100.0, 1.0, 0.0, true)
///     .solve(&SolverConfig::default());
/// assert!(result.converged);
/// assert!((result.iv - 0.20).abs() < 1e-6);
/// ```
///
/// New fields may be added in future minor versions.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct IvQuery {
    /// Observed option market price.
    pub market_price: f64,
    /// Forward / futures price.
    pub f: f64,
    /// Strike.
    pub k: f64,
    /// Time to expiry, in years.
    pub t: f64,
    /// Risk-free rate (annualized, continuously compounded).
    pub r: f64,
    /// `true` for a call quote, `false` for a put.
    pub is_call: bool,
}

impl IvQuery {
    /// Construct a quote from market price, forward, strike, time, rate, flag.
    #[must_use]
    pub const fn new(market_price: f64, f: f64, k: f64, t: f64, r: f64, is_call: bool) -> Self {
        Self {
            market_price,
            f,
            k,
            t,
            r,
            is_call,
        }
    }

    /// Solve for implied volatility. Like [`solve_iv`], this returns a
    /// [`SolverResult`] — **check `converged` before consuming `iv`** (it is
    /// `NaN` on every non-converged path).
    #[must_use]
    pub fn solve(&self, config: &SolverConfig) -> SolverResult {
        solve_iv(
            self.market_price,
            self.f,
            self.k,
            self.t,
            self.r,
            self.is_call,
            config,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_inputs_match_free_functions() {
        let (f, k, t, s, r) = (100.0, 105.0, 0.5, 0.25, 0.03);
        let bi = BlackInputs::new(f, k, t, s, r);
        assert_eq!(bi.call_price(), call_price(f, k, t, s, r));
        assert_eq!(bi.put_price(), put_price(f, k, t, s, r));
        assert_eq!(bi.price(true), price(f, k, t, s, r, true));
        assert_eq!(bi.price(false), price(f, k, t, s, r, false));
        assert_eq!(bi.vega(), vega(f, k, t, s, r));
        assert_eq!(bi.d1_d2(), d1_d2(f, k, t, s));
        assert_eq!(bi.intrinsic_value(true), intrinsic_value(f, k, true));

        let g = bi.greeks(true);
        let g2 = compute_greeks(f, k, t, s, r, true);
        assert_eq!(g.delta, g2.delta);
        assert_eq!(g.gamma, g2.gamma);
        assert_eq!(g.vega, g2.vega);
        assert_eq!(g.theta, g2.theta);
        assert_eq!(g.rho, g2.rho);
    }

    #[test]
    fn iv_query_round_trip() {
        let cfg = SolverConfig::default();
        let market = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
        let result = IvQuery::new(market, 100.0, 100.0, 1.0, 0.0, true).solve(&cfg);
        assert!(result.converged);
        assert!((result.iv - 0.20).abs() < 1e-6);
    }

    #[test]
    fn const_construction() {
        // Both constructors are usable in a `const` context (this compiles).
        const BI: BlackInputs = BlackInputs::new(100.0, 100.0, 1.0, 0.20, 0.0);
        const Q: IvQuery = IvQuery::new(7.96, 100.0, 100.0, 1.0, 0.0, true);
        // Exercise the values at runtime (avoids asserting on constants).
        let (bi, q) = (BI, Q);
        assert_eq!(bi.k, 100.0);
        assert!(q.is_call);
    }
}
