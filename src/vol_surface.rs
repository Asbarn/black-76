//! Per-expiry implied volatility smile with linear-in-strike interpolation.
//!
//! Construct a [`VolSmile`] from raw (strike, IV) observations with quality
//! filtering, linear interpolation between observed points, and flat
//! extrapolation beyond boundary strikes. Quality tiers
//! ([`SmileQuality::Good`] / [`Minimum`](SmileQuality::Minimum) /
//! [`Degraded`](SmileQuality::Degraded) / [`Empty`](SmileQuality::Empty))
//! reflect surface reliability for downstream confidence.
//!
//! # Arbitrage
//!
//! Linear-in-strike interpolation and flat-wing extrapolation are **not**
//! guaranteed arbitrage-free: the implied risk-neutral density (`∂²C/∂K²`, per
//! Breeden-Litzenberger) can go negative between nodes, so digitals derived
//! from this smile may be locally inconsistent. This is a lightweight smoothing
//! utility — for arbitrage-free surfaces use a total-variance or SVI/SABR
//! parameterization (Gatheral, *The Volatility Surface*) or arbitrage-free
//! smoothing (Fengler 2009).
//!
//! # Quick start
//!
//! ```
//! use black_76::vol_surface::{SmilePoint, VolSmile, VolSurfaceConfig};
//!
//! let config = VolSurfaceConfig::default();
//! let points = vec![
//!     SmilePoint::new(90.0, 0.32, 0.31, 0.33),
//!     SmilePoint::new(95.0, 0.28, 0.27, 0.29),
//!     SmilePoint::new(100.0, 0.25, 0.245, 0.255),
//!     SmilePoint::new(105.0, 0.27, 0.265, 0.275),
//!     SmilePoint::new(110.0, 0.31, 0.30, 0.32),
//! ];
//! let smile = VolSmile::new(None, points, &config, 100.0);
//! let iv = smile.interpolate(102.5).unwrap();
//! assert!((iv - 0.26).abs() < 1e-9);
//! ```

// ---------------------------------------------------------------------------
// VolSurfaceConfig
// ---------------------------------------------------------------------------

/// Configuration for vol-surface construction.
///
/// Controls quality filtering and tier thresholds when building a [`VolSmile`]
/// from raw observations. Construct via [`VolSurfaceConfig::default`] for
/// sensible defaults, or via [`VolSurfaceConfig::builder`] for selective
/// overrides:
///
/// ```
/// use black_76::vol_surface::VolSurfaceConfig;
///
/// let config = VolSurfaceConfig::builder()
///     .min_usable_strikes(2)
///     .good_strike_count(7)
///     .max_iv_spread_filter(0.30)
///     .build();
/// assert_eq!(config.min_usable_strikes, 2);
/// ```
///
/// New fields may be added in future minor versions.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[non_exhaustive]
pub struct VolSurfaceConfig {
    /// Minimum usable strikes per expiry to interpolate (below this, falls
    /// back to flat ATM vol).
    pub min_usable_strikes: usize,
    /// Strike count at or above which the smile is rated [`SmileQuality::Good`].
    pub good_strike_count: usize,
    /// Maximum IV bid-ask spread to retain a strike. Strikes with wider spread
    /// are excluded from the smile.
    pub max_iv_spread_filter: f64,
}

impl Default for VolSurfaceConfig {
    fn default() -> Self {
        Self {
            min_usable_strikes: 3,
            good_strike_count: 5,
            max_iv_spread_filter: 0.50,
        }
    }
}

impl VolSurfaceConfig {
    /// Returns a fresh builder seeded with the default config.
    pub fn builder() -> VolSurfaceConfigBuilder {
        VolSurfaceConfigBuilder {
            inner: VolSurfaceConfig::default(),
        }
    }
}

/// Builder for [`VolSurfaceConfig`].
///
/// Each setter consumes `self` and returns a new builder; call
/// [`build`](Self::build) to produce the final config. All setters are
/// `const fn`, so a config can be assembled in a `const` context.
#[derive(Debug, Clone)]
#[must_use = "a builder does nothing unless you call `.build()`"]
pub struct VolSurfaceConfigBuilder {
    inner: VolSurfaceConfig,
}

impl VolSurfaceConfigBuilder {
    /// Sets `min_usable_strikes`.
    pub const fn min_usable_strikes(mut self, value: usize) -> Self {
        self.inner.min_usable_strikes = value;
        self
    }

    /// Sets `good_strike_count`.
    pub const fn good_strike_count(mut self, value: usize) -> Self {
        self.inner.good_strike_count = value;
        self
    }

    /// Sets `max_iv_spread_filter`.
    pub const fn max_iv_spread_filter(mut self, value: f64) -> Self {
        self.inner.max_iv_spread_filter = value;
        self
    }

    /// Consumes the builder and returns the configured [`VolSurfaceConfig`].
    #[must_use]
    pub const fn build(self) -> VolSurfaceConfig {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// SmilePoint
// ---------------------------------------------------------------------------

/// A single observed point on the vol smile.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct SmilePoint {
    /// Strike price.
    pub strike: f64,
    /// Mid implied volatility (annualized).
    pub iv: f64,
    /// Bid-side implied volatility.
    pub bid_iv: f64,
    /// Ask-side implied volatility.
    pub ask_iv: f64,
    /// IV bid-ask spread (`ask_iv - bid_iv`).
    pub iv_spread: f64,
}

impl SmilePoint {
    /// Construct a smile point. The bid-ask spread is computed as
    /// `ask_iv - bid_iv`.
    #[must_use]
    pub fn new(strike: f64, iv: f64, bid_iv: f64, ask_iv: f64) -> Self {
        Self {
            strike,
            iv,
            bid_iv,
            ask_iv,
            iv_spread: ask_iv - bid_iv,
        }
    }
}

// ---------------------------------------------------------------------------
// SmileQuality
// ---------------------------------------------------------------------------

/// Quality tier reflecting vol-smile reliability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SmileQuality {
    /// `good_strike_count` or more usable strikes — reliable interpolation.
    Good,
    /// Between `min_usable_strikes` and `good_strike_count - 1` usable strikes
    /// — minimum for interpolation.
    Minimum,
    /// Fewer than `min_usable_strikes` — falls back to flat ATM vol.
    Degraded,
    /// Zero usable strikes — no data.
    Empty,
}

// ---------------------------------------------------------------------------
// VolSmile
// ---------------------------------------------------------------------------

/// Per-expiry implied-volatility smile with quality filtering.
///
/// Points are always sorted by strike ascending. Quality filtering excludes
/// strikes with excessive IV bid-ask spread or non-positive IV.
///
/// The `expiry` field is a free-form label — pass `None` if you don't track
/// expiry timestamps, or `Some(unix_seconds)` if you do. The smile math
/// itself does not consume the expiry value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct VolSmile {
    /// Expiry label (UNIX seconds), or `None` if untracked.
    pub expiry: Option<i64>,
    /// Usable smile points, sorted by strike ascending.
    pub points: Vec<SmilePoint>,
    /// Excluded strikes with reasons (strike, reason).
    pub excluded: Vec<(f64, String)>,
    /// Quality tier based on remaining point count.
    pub quality: SmileQuality,
    /// ATM implied volatility (nearest to forward price).
    pub atm_iv: Option<f64>,
}

impl VolSmile {
    /// Construct a vol smile from raw observations with quality filtering.
    ///
    /// Filters out points with excessive IV spread or non-positive IV,
    /// sorts the remaining points by strike, determines the quality tier,
    /// and identifies ATM IV as the point nearest to the forward price.
    #[must_use]
    pub fn new(
        expiry: Option<i64>,
        raw_points: Vec<SmilePoint>,
        config: &VolSurfaceConfig,
        forward_price: f64,
    ) -> Self {
        let mut points = Vec::with_capacity(raw_points.len());
        let mut excluded = Vec::new();

        for p in raw_points {
            if p.iv <= 0.0 {
                excluded.push((p.strike, "non-positive IV".to_string()));
                continue;
            }

            if p.iv_spread > config.max_iv_spread_filter {
                excluded.push((
                    p.strike,
                    format!(
                        "iv_spread={:.2} exceeds max {:.2}",
                        p.iv_spread, config.max_iv_spread_filter
                    ),
                ));
                continue;
            }

            points.push(p);
        }

        points.sort_by(|a, b| {
            a.strike
                .partial_cmp(&b.strike)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let atm_iv = if points.is_empty() {
            None
        } else {
            let mut closest = &points[0];
            let mut min_dist = (closest.strike - forward_price).abs();
            for p in &points[1..] {
                let dist = (p.strike - forward_price).abs();
                if dist < min_dist {
                    min_dist = dist;
                    closest = p;
                }
            }
            Some(closest.iv)
        };

        let count = points.len();
        let quality = if count == 0 {
            SmileQuality::Empty
        } else if count < config.min_usable_strikes {
            SmileQuality::Degraded
        } else if count >= config.good_strike_count {
            SmileQuality::Good
        } else {
            SmileQuality::Minimum
        };

        Self {
            expiry,
            points,
            excluded,
            quality,
            atm_iv,
        }
    }

    /// Number of usable points in the smile.
    #[must_use]
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// True if no usable points remain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Interpolate implied volatility at an arbitrary strike.
    ///
    /// - [`Empty`](SmileQuality::Empty) quality: returns `None`.
    /// - [`Degraded`](SmileQuality::Degraded) quality with ATM IV: returns
    ///   flat ATM vol for any strike.
    /// - Single point: returns that point's IV for any strike.
    /// - Below the minimum strike: flat extrapolation (first point's IV).
    /// - Above the maximum strike: flat extrapolation (last point's IV).
    /// - Between two points: linear interpolation.
    #[must_use]
    pub fn interpolate(&self, strike: f64) -> Option<f64> {
        if self.quality == SmileQuality::Empty {
            return None;
        }

        if self.quality == SmileQuality::Degraded {
            if let Some(atm) = self.atm_iv {
                return Some(atm);
            }
            return self.points.first().map(|p| p.iv);
        }

        if self.points.len() == 1 {
            return Some(self.points[0].iv);
        }

        let first = &self.points[0];
        let last = &self.points[self.points.len() - 1];

        if strike <= first.strike {
            return Some(first.iv);
        }

        if strike >= last.strike {
            return Some(last.iv);
        }

        let idx = self.points.partition_point(|p| p.strike < strike);

        let upper = &self.points[idx];
        let lower = &self.points[idx - 1];

        if (upper.strike - strike).abs() < f64::EPSILON {
            return Some(upper.iv);
        }
        if (lower.strike - strike).abs() < f64::EPSILON {
            return Some(lower.iv);
        }

        let t = (strike - lower.strike) / (upper.strike - lower.strike);
        let iv = lower.iv + (upper.iv - lower.iv) * t;
        Some(iv)
    }

    /// Find the nearest observed strikes bracketing the target strike.
    ///
    /// Returns `(k_lower, k_upper)` where `k_lower < target < k_upper`.
    /// If the target lands exactly on an observed strike, uses the adjacent
    /// strikes on both sides.
    ///
    /// Returns `None` if the target is below all or above all observed
    /// strikes (cannot bracket), or if fewer than two points exist.
    #[must_use]
    pub fn nearest_bracket(&self, target_strike: f64) -> Option<(f64, f64)> {
        if self.points.len() < 2 {
            return None;
        }

        let first = self.points[0].strike;
        let last = self.points[self.points.len() - 1].strike;

        if target_strike <= first || target_strike >= last {
            return None;
        }

        let idx = self.points.partition_point(|p| p.strike < target_strike);

        // Scale-relative equality so an on-grid strike is recognized at crypto
        // magnitudes (e.g. 100_000), where an absolute `f64::EPSILON` never
        // matches a strike that was computed rather than typed (audit L-3).
        let strike_eq_tol = 1e-9 * target_strike.abs().max(1.0);

        if idx < self.points.len()
            && (self.points[idx].strike - target_strike).abs() <= strike_eq_tol
        {
            // Exact hit on an observed strike: use the neighbors on BOTH sides
            // so the spread straddles the target. On a non-uniform grid this
            // widens the spread; the caller (`call_spread_probability`) caps
            // and recenters it on the target.
            if idx == 0 || idx >= self.points.len() - 1 {
                return None;
            }
            return Some((self.points[idx - 1].strike, self.points[idx + 1].strike));
        }

        if idx == 0 || idx >= self.points.len() {
            return None;
        }

        Some((self.points[idx - 1].strike, self.points[idx].strike))
    }

    /// Compute skew at a given strike: `interpolate(strike) - atm_iv`.
    ///
    /// Returns `None` if ATM IV is unavailable or interpolation fails.
    #[must_use]
    pub fn skew_at(&self, strike: f64) -> Option<f64> {
        let atm = self.atm_iv?;
        let strike_iv = self.interpolate(strike)?;
        Some(strike_iv - atm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> VolSurfaceConfig {
        VolSurfaceConfig::default()
    }

    fn make_point(strike: f64, iv: f64, spread: f64) -> SmilePoint {
        SmilePoint::new(strike, iv, iv - spread / 2.0, iv + spread / 2.0)
    }

    #[test]
    fn construction_good_quality() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        let smile = VolSmile::new(Some(1_750_000_000), points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Good);
        assert_eq!(smile.points.len(), 5);
        assert!(smile.excluded.is_empty());
        assert!((smile.atm_iv.unwrap() - 0.50).abs() < f64::EPSILON);
    }

    #[test]
    fn construction_excludes_wide_spread() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.80),
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.70),
            make_point(110000.0, 0.58, 0.06),
        ];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        assert_eq!(smile.points.len(), 3);
        assert_eq!(smile.excluded.len(), 2);
        assert_eq!(smile.quality, SmileQuality::Minimum);

        assert!(smile.excluded[0].1.contains("iv_spread="));
        assert!(smile.excluded[0].1.contains("exceeds max"));
    }

    #[test]
    fn construction_degraded_quality() {
        let config = default_config();
        let points = vec![
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
        ];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Degraded);
        assert_eq!(smile.points.len(), 2);
        assert!(smile.atm_iv.is_some());
    }

    #[test]
    fn construction_sorted_by_strike() {
        let config = default_config();
        let points = vec![
            make_point(110000.0, 0.58, 0.06),
            make_point(90000.0, 0.60, 0.05),
            make_point(105000.0, 0.52, 0.04),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
        ];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        let strikes: Vec<f64> = smile.points.iter().map(|p| p.strike).collect();
        assert_eq!(
            strikes,
            vec![90000.0, 95000.0, 100000.0, 105000.0, 110000.0]
        );
    }

    #[test]
    fn construction_empty() {
        let config = default_config();
        let smile = VolSmile::new(None, Vec::new(), &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Empty);
        assert!(smile.atm_iv.is_none());
        assert!(smile.is_empty());
    }

    #[test]
    fn construction_excludes_non_positive_iv() {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.0, 0.04),
            make_point(100000.0, -0.10, 0.03),
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        assert_eq!(smile.points.len(), 3);
        assert_eq!(smile.excluded.len(), 2);
        assert!(
            smile
                .excluded
                .iter()
                .all(|(_, reason)| reason == "non-positive IV")
        );
        assert_eq!(smile.quality, SmileQuality::Minimum);
    }

    fn make_good_smile() -> VolSmile {
        let config = default_config();
        let points = vec![
            make_point(90000.0, 0.60, 0.05),
            make_point(95000.0, 0.55, 0.04),
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
            make_point(110000.0, 0.58, 0.06),
        ];
        VolSmile::new(None, points, &config, 100000.0)
    }

    #[test]
    fn interpolate_exact_strike() {
        let smile = make_good_smile();
        let iv = smile.interpolate(100000.0).unwrap();
        assert!((iv - 0.50).abs() < 1e-10);

        let iv_low = smile.interpolate(90000.0).unwrap();
        assert!((iv_low - 0.60).abs() < 1e-10);
    }

    #[test]
    fn interpolate_between_strikes() {
        let smile = make_good_smile();
        let iv = smile.interpolate(92500.0).unwrap();
        let expected = 0.60 + (0.55 - 0.60) * (92500.0 - 90000.0) / (95000.0 - 90000.0);
        assert!((iv - expected).abs() < 1e-10);
        assert!(iv > 0.55 && iv < 0.60);
    }

    #[test]
    fn extrapolate_below() {
        let smile = make_good_smile();
        let iv = smile.interpolate(80000.0).unwrap();
        assert!((iv - 0.60).abs() < 1e-10);
    }

    #[test]
    fn extrapolate_above() {
        let smile = make_good_smile();
        let iv = smile.interpolate(120000.0).unwrap();
        assert!((iv - 0.58).abs() < 1e-10);
    }

    #[test]
    fn nearest_bracket_between() {
        let smile = make_good_smile();
        let (lower, upper) = smile.nearest_bracket(97000.0).unwrap();
        assert!((lower - 95000.0).abs() < f64::EPSILON);
        assert!((upper - 100000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn nearest_bracket_out_of_range() {
        let smile = make_good_smile();
        assert!(smile.nearest_bracket(80000.0).is_none());
        assert!(smile.nearest_bracket(120000.0).is_none());
        assert!(smile.nearest_bracket(90000.0).is_none());
        assert!(smile.nearest_bracket(110000.0).is_none());
    }

    #[test]
    fn nearest_bracket_exact_strike() {
        let smile = make_good_smile();
        let (lower, upper) = smile.nearest_bracket(100000.0).unwrap();
        assert!((lower - 95000.0).abs() < f64::EPSILON);
        assert!((upper - 105000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn nearest_bracket_near_grid_target_recognized_at_scale() {
        let smile = make_good_smile(); // strikes 90_000 ..= 110_000
        // A target a hair below the 100_000 node (as if computed with rounding)
        // is recognized as the on-grid strike thanks to the scale-relative
        // tolerance (audit L-3); an absolute f64::EPSILON would miss it and
        // return (95_000, 100_000) instead.
        let (lower, upper) = smile.nearest_bracket(100_000.0 - 1e-5).unwrap();
        assert!((lower - 95_000.0).abs() < f64::EPSILON);
        assert!((upper - 105_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn nearest_bracket_uneven_grid_exact_hit() {
        let config = default_config();
        let points = vec![
            make_point(100.0, 0.30, 0.02),
            make_point(101.0, 0.29, 0.02),
            make_point(150.0, 0.40, 0.02),
        ];
        let smile = VolSmile::new(None, points, &config, 101.0);
        // Exact hit at 101 -> neighbors on both sides (100, 150); the spread is
        // wide because the grid is uneven.
        let (lower, upper) = smile.nearest_bracket(101.0).unwrap();
        assert!((lower - 100.0).abs() < f64::EPSILON);
        assert!((upper - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn skew_at_various_strikes() {
        let smile = make_good_smile();

        let skew_atm = smile.skew_at(100000.0).unwrap();
        assert!(skew_atm.abs() < 1e-10);

        let skew_low = smile.skew_at(90000.0).unwrap();
        assert!((skew_low - 0.10).abs() < 1e-10);

        let skew_high = smile.skew_at(110000.0).unwrap();
        assert!((skew_high - 0.08).abs() < 1e-10);
    }

    #[test]
    fn degraded_returns_flat_atm() {
        let config = default_config();
        let points = vec![
            make_point(100000.0, 0.50, 0.03),
            make_point(105000.0, 0.52, 0.04),
        ];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        assert_eq!(smile.quality, SmileQuality::Degraded);

        let iv_low = smile.interpolate(80000.0).unwrap();
        let iv_mid = smile.interpolate(100000.0).unwrap();
        let iv_high = smile.interpolate(120000.0).unwrap();
        assert!((iv_low - 0.50).abs() < 1e-10);
        assert!((iv_mid - 0.50).abs() < 1e-10);
        assert!((iv_high - 0.50).abs() < 1e-10);
    }

    #[test]
    fn empty_returns_none() {
        let config = default_config();
        let smile = VolSmile::new(None, Vec::new(), &config, 100000.0);

        assert!(smile.interpolate(100000.0).is_none());
        assert!(smile.nearest_bracket(100000.0).is_none());
        assert!(smile.skew_at(100000.0).is_none());
    }

    #[test]
    fn single_point_returns_its_iv() {
        let config = VolSurfaceConfig::builder().min_usable_strikes(1).build();
        let points = vec![make_point(100000.0, 0.50, 0.03)];
        let smile = VolSmile::new(None, points, &config, 100000.0);

        let iv = smile.interpolate(80000.0).unwrap();
        assert!((iv - 0.50).abs() < 1e-10);
        let iv = smile.interpolate(120000.0).unwrap();
        assert!((iv - 0.50).abs() < 1e-10);
    }

    #[test]
    fn interpolation_monotonicity() {
        let smile = make_good_smile();
        let strikes = &smile.points;
        for w in strikes.windows(2) {
            let (k_lo, iv_lo) = (w[0].strike, w[0].iv);
            let (k_hi, iv_hi) = (w[1].strike, w[1].iv);
            let lo_iv = iv_lo.min(iv_hi);
            let hi_iv = iv_lo.max(iv_hi);

            for i in 1..10 {
                let frac = i as f64 / 10.0;
                let k = k_lo + (k_hi - k_lo) * frac;
                let iv = smile.interpolate(k).unwrap();
                assert!(iv >= lo_iv - 1e-10 && iv <= hi_iv + 1e-10);
            }
        }
    }

    #[test]
    fn builder_overrides() {
        let config = VolSurfaceConfig::builder()
            .min_usable_strikes(2)
            .good_strike_count(7)
            .max_iv_spread_filter(0.30)
            .build();
        assert_eq!(config.min_usable_strikes, 2);
        assert_eq!(config.good_strike_count, 7);
        assert!((config.max_iv_spread_filter - 0.30).abs() < f64::EPSILON);
    }
}
