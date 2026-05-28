//! Solver configuration.
//!
//! The default values (50 NR iterations, 1e-8 price tolerance, IV in [0.01, 5.0])
//! are tuned for crypto options pricing but work generally. Use
//! [`SolverConfig::builder`] for fluent construction.

/// Configuration for the implied-volatility solver.
///
/// Construct via [`SolverConfig::default`] for sensible defaults, or via
/// [`SolverConfig::builder`] for selective overrides:
///
/// ```
/// use black_76::SolverConfig;
///
/// let cfg = SolverConfig::builder()
///     .iv_min(0.001)
///     .iv_max(10.0)
///     .price_tolerance(1e-10)
///     .build();
/// assert_eq!(cfg.iv_max, 10.0);
/// ```
///
/// New fields may be added in future minor versions.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[non_exhaustive]
pub struct SolverConfig {
    /// Maximum Newton-Raphson iterations before switching to Brent.
    pub nr_max_iterations: u32,
    /// Price tolerance for convergence (`|model - market| < tolerance`).
    pub price_tolerance: f64,
    /// Minimum vega to continue Newton-Raphson iterations. Below this, the
    /// solver falls back to Brent's method to avoid the explosive step
    /// `Δsigma = (price - target) / vega` when vega → 0 (deep OTM, near expiry).
    pub vega_floor: f64,
    /// Minimum allowed IV (annualized). The solver clamps every NR step to
    /// `[iv_min, iv_max]`. Default 0.01 (1%).
    pub iv_min: f64,
    /// Maximum allowed IV (annualized). Default 5.0 (500%).
    pub iv_max: f64,
    /// Maximum Brent's-method iterations.
    pub brent_max_iterations: u32,
    /// Near-expiry cutoff in hours. Below this, the solver bypasses NR/Brent
    /// and returns an intrinsic-pricing result (`iv = 0.0, converged = true`).
    pub near_expiry_cutoff_hours: f64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            nr_max_iterations: 50,
            price_tolerance: 1e-8,
            vega_floor: 1e-10,
            iv_min: 0.01,
            iv_max: 5.0,
            brent_max_iterations: 100,
            near_expiry_cutoff_hours: 2.0,
        }
    }
}

impl SolverConfig {
    /// Returns a fresh builder seeded with the default config.
    pub fn builder() -> SolverConfigBuilder {
        SolverConfigBuilder {
            inner: SolverConfig::default(),
        }
    }
}

/// Builder for [`SolverConfig`].
///
/// Each setter consumes `self` and returns a new builder; call [`build`](Self::build)
/// to produce the final config.
#[derive(Debug, Clone)]
pub struct SolverConfigBuilder {
    inner: SolverConfig,
}

impl SolverConfigBuilder {
    /// Sets `nr_max_iterations`.
    pub fn nr_max_iterations(mut self, value: u32) -> Self {
        self.inner.nr_max_iterations = value;
        self
    }

    /// Sets `price_tolerance`.
    pub fn price_tolerance(mut self, value: f64) -> Self {
        self.inner.price_tolerance = value;
        self
    }

    /// Sets `vega_floor`.
    pub fn vega_floor(mut self, value: f64) -> Self {
        self.inner.vega_floor = value;
        self
    }

    /// Sets `iv_min`.
    pub fn iv_min(mut self, value: f64) -> Self {
        self.inner.iv_min = value;
        self
    }

    /// Sets `iv_max`.
    pub fn iv_max(mut self, value: f64) -> Self {
        self.inner.iv_max = value;
        self
    }

    /// Sets `brent_max_iterations`.
    pub fn brent_max_iterations(mut self, value: u32) -> Self {
        self.inner.brent_max_iterations = value;
        self
    }

    /// Sets `near_expiry_cutoff_hours`.
    pub fn near_expiry_cutoff_hours(mut self, value: f64) -> Self {
        self.inner.near_expiry_cutoff_hours = value;
        self
    }

    /// Consumes the builder and returns the configured [`SolverConfig`].
    pub fn build(self) -> SolverConfig {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let cfg = SolverConfig::default();
        assert_eq!(cfg.nr_max_iterations, 50);
        assert_eq!(cfg.iv_min, 0.01);
        assert_eq!(cfg.iv_max, 5.0);
    }

    #[test]
    fn builder_overrides() {
        let cfg = SolverConfig::builder()
            .iv_min(0.005)
            .iv_max(8.0)
            .price_tolerance(1e-10)
            .build();
        assert_eq!(cfg.iv_min, 0.005);
        assert_eq!(cfg.iv_max, 8.0);
        assert_eq!(cfg.price_tolerance, 1e-10);
        // Untouched defaults preserved
        assert_eq!(cfg.nr_max_iterations, 50);
    }
}
