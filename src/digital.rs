//! Risk-neutral probability extraction via call-spread replication and N(d2).
//!
//! Two independent estimators for `P_Q(F_T > K)` under the risk-neutral
//! measure:
//!
//! 1. **Call-spread replication** (primary): a tight call spread centered on
//!    the target strike, priced with the smile's interpolated (skew-aware) IV
//!    at each leg. The discounted price difference is rescaled by `exp(rT)` to
//!    recover the undiscounted probability (see HIGH-A-02), and centered on the
//!    target to avoid the strike-location bias of audit M-2.
//! 2. **N(d2)** (baseline): closed-form Black-76 risk-neutral probability
//!    with skew adjustment (`strike_iv − atm_iv`). Always available when
//!    the smile can interpolate at the target strike.
//!
//! Both are computed and their disagreement is reported for downstream
//! confidence scoring.
//!
//! # Quick start
//!
//! ```
//! use black_76::vol_surface::{SmilePoint, VolSmile, VolSurfaceConfig};
//! use black_76::digital::extract_probabilities;
//!
//! let config = VolSurfaceConfig::default();
//! let points = vec![
//!     SmilePoint::new(90.0, 0.30, 0.295, 0.305),
//!     SmilePoint::new(95.0, 0.28, 0.275, 0.285),
//!     SmilePoint::new(100.0, 0.25, 0.245, 0.255),
//!     SmilePoint::new(105.0, 0.27, 0.265, 0.275),
//!     SmilePoint::new(110.0, 0.30, 0.295, 0.305),
//! ];
//! let smile = VolSmile::new(None, points, &config, 100.0);
//!
//! // ATM probability with a 10.0 epsilon budget
//! let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, 10.0).unwrap();
//! assert!(extraction.primary_probability > 0.4 && extraction.primary_probability < 0.6);
//! ```

use statrs::distribution::{ContinuousCDF, Normal};

use crate::pricing::{call_price, d1_d2};
use crate::vol_surface::VolSmile;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Output of call-spread replication probability extraction.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct CallSpreadResult {
    /// Extracted probability `P_Q(F_T > K)`.
    pub probability: f64,
    /// Epsilon used (half the distance between bracket strikes).
    pub epsilon_used: f64,
    /// Lower bracket strike.
    pub k_lower: f64,
    /// Upper bracket strike.
    pub k_upper: f64,
}

/// Output of N(d2) probability extraction.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Nd2Result {
    /// Extracted probability `P_Q(F_T > K)` via `N(d2)`.
    pub probability: f64,
    /// Skew adjustment: `strike_iv − atm_iv` (0 if ATM IV unavailable).
    pub skew_adjustment: f64,
}

/// Combined probability extraction output.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct ProbabilityExtraction {
    /// Primary probability estimate (from whichever method was selected).
    pub primary_probability: f64,
    /// Which method produced `primary_probability`.
    pub primary_method: ProbabilityMethod,
    /// Call-spread result (`None` if epsilon exceeded `max_epsilon` or no
    /// bracket).
    pub call_spread: Option<CallSpreadResult>,
    /// `N(d2)` result (present whenever IV interpolation succeeds at the
    /// target strike).
    pub nd2: Nd2Result,
    /// Absolute disagreement `|call_spread.probability − nd2.probability|`
    /// when both methods are present; otherwise zero.
    pub method_disagreement: f64,
    /// Skew adjustment from N(d2) (`strike_iv − atm_iv`).
    pub skew_adjustment: f64,
}

/// Which probability method was used as primary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ProbabilityMethod {
    /// Call-spread replication using observed bracket strikes.
    CallSpreadReplication,
    /// Black-76 `N(d2)` with skew adjustment.
    Nd2SkewAdjusted,
}

// ---------------------------------------------------------------------------
// Call-spread replication
// ---------------------------------------------------------------------------

/// Compute `P_Q(F_T > target_strike)` via call-spread replication, centered on
/// the target strike.
///
/// Steps out symmetrically from `target_strike` by `ε` (the local observed
/// half-spacing, clamped to keep both legs inside the smile), prices a call at
/// `target ± ε` using the IV interpolated at each leg, and recovers the
/// risk-neutral probability from the discount-corrected central difference:
///
/// ```text
/// P_Q(F_T > K) ≈ (c(K − ε) − c(K + ε)) / (2ε · exp(−rT))
/// ```
///
/// Centering on the target — rather than on the midpoint of two observed
/// strikes — removes the strike-location bias of audit M-2.
///
/// Returns `None` when:
/// - the smile cannot bracket `target_strike` (target outside observed range);
/// - the local observed half-spacing exceeds `max_epsilon` (smile too sparse —
///   the caller falls back to `N(d2)`);
/// - IV interpolation fails at either leg;
/// - the usable step `ε` is non-positive;
/// - the raw probability falls outside `[0, 1]` — a smile arbitrage (e.g.
///   `c(K − ε) < c(K + ε)`) reported as unavailable rather than silently
///   clamped (audit M-2).
fn call_spread_probability(
    target_strike: f64,
    smile: &VolSmile,
    forward: f64,
    time_to_expiry: f64,
    rate: f64,
    max_epsilon: f64,
) -> Option<CallSpreadResult> {
    // Use the local observed bracket only to size the step and to gate on
    // smile density; the actual finite difference is centered on the target.
    let (k_lower, k_upper) = smile.nearest_bracket(target_strike)?;

    let half_spacing = (k_upper - k_lower) / 2.0;
    if half_spacing > max_epsilon {
        // Local smile spacing too wide for a reliable call spread; the caller
        // falls back to N(d2).
        return None;
    }

    // Center the difference on the target (audit M-2): the central difference
    // estimates the CDF at the MIDPOINT of its two legs, so evaluating at
    // observed grid strikes returns P(F > bracket-midpoint), not P(F > target).
    // Keep both legs inside the interpolation range.
    let first = smile.points.first()?.strike;
    let last = smile.points.last()?.strike;
    let epsilon = half_spacing
        .min(target_strike - first)
        .min(last - target_strike);
    if epsilon <= 0.0 {
        return None;
    }

    let k_low = target_strike - epsilon;
    let k_high = target_strike + epsilon;

    let iv_low = smile.interpolate(k_low)?;
    let iv_high = smile.interpolate(k_high)?;

    let c_low = call_price(forward, k_low, time_to_expiry, iv_low, rate);
    let c_high = call_price(forward, k_high, time_to_expiry, iv_high, rate);

    // HIGH-A-02: undo discounting (the `/ df`) so the result is a probability,
    // not a discounted-probability proxy.
    let df = (-rate * time_to_expiry).exp();
    let raw_prob = (c_low - c_high) / (2.0 * epsilon) / df;

    // Audit M-2: do NOT silently clamp. A raw probability outside [0, 1] (or
    // `c_low < c_high`) signals a smile arbitrage or numerical breakdown;
    // report it as unavailable rather than returning a plausible-looking
    // 0.0 / 1.0. A tiny float-noise overshoot is tolerated, then clamped.
    if !raw_prob.is_finite() || !(-1e-9..=1.0 + 1e-9).contains(&raw_prob) {
        return None;
    }
    let prob = raw_prob.clamp(0.0, 1.0);

    Some(CallSpreadResult {
        probability: prob,
        epsilon_used: epsilon,
        k_lower: k_low,
        k_upper: k_high,
    })
}

// ---------------------------------------------------------------------------
// N(d2)
// ---------------------------------------------------------------------------

/// Compute `P_Q(F_T > strike)` via Black-76 `N(d2)`.
///
/// The skew adjustment is `strike_iv − atm_iv`; pass `None` for `atm_iv`
/// to suppress it.
fn nd2_probability(
    forward: f64,
    strike: f64,
    time_to_expiry: f64,
    strike_iv: f64,
    atm_iv: Option<f64>,
) -> Nd2Result {
    let (_, d2) = d1_d2(forward, strike, time_to_expiry, strike_iv);
    let norm = Normal::standard();
    let probability = norm.cdf(d2);
    let skew_adjustment = atm_iv.map(|atm| strike_iv - atm).unwrap_or(0.0);

    Nd2Result {
        probability,
        skew_adjustment,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract `P_Q(F_T > target_strike)` using both methods and report which
/// served as primary.
///
/// Call-spread replication is preferred whenever it succeeds. When the
/// bracket half-width exceeds `max_epsilon` (or no bracket exists), the
/// function falls back to `N(d2)` with skew adjustment. Both estimators run
/// whenever possible, and the absolute disagreement is reported.
///
/// Requires `time_to_expiry > 0`. Returns `None` when the option has expired
/// (`time_to_expiry <= 0`, where the risk-neutral probability degenerates to
/// the intrinsic indicator and `N(d2)` is undefined at the money) or when IV
/// cannot be interpolated at `target_strike` (so `N(d2)` is also unavailable).
#[must_use]
pub fn extract_probabilities(
    target_strike: f64,
    smile: &VolSmile,
    forward: f64,
    time_to_expiry: f64,
    rate: f64,
    max_epsilon: f64,
) -> Option<ProbabilityExtraction> {
    // At/after expiry `d1_d2` forms `v = σ√T = 0`, so `N(d2)` is NaN at the
    // money (0/0) and ±inf-driven 0/1 off the money — never a usable
    // probability. Mirror the `t <= 0` intrinsic guards in `pricing`/`greeks`
    // by reporting the result as unavailable rather than leaking NaN through
    // `nd2.probability` / `method_disagreement`.
    if time_to_expiry <= 0.0 {
        return None;
    }

    let call_spread = call_spread_probability(
        target_strike,
        smile,
        forward,
        time_to_expiry,
        rate,
        max_epsilon,
    );

    let strike_iv = smile.interpolate(target_strike)?;

    let nd2 = nd2_probability(
        forward,
        target_strike,
        time_to_expiry,
        strike_iv,
        smile.atm_iv,
    );

    let (primary_probability, primary_method) = if let Some(ref cs) = call_spread {
        (cs.probability, ProbabilityMethod::CallSpreadReplication)
    } else {
        (nd2.probability, ProbabilityMethod::Nd2SkewAdjusted)
    };

    let method_disagreement = if let Some(ref cs) = call_spread {
        (cs.probability - nd2.probability).abs()
    } else {
        0.0
    };

    let skew_adjustment = nd2.skew_adjustment;

    Some(ProbabilityExtraction {
        primary_probability,
        primary_method,
        call_spread,
        nd2,
        method_disagreement,
        skew_adjustment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vol_surface::{SmilePoint, VolSmile, VolSurfaceConfig};

    fn make_point(strike: f64, iv: f64, spread: f64) -> SmilePoint {
        SmilePoint::new(strike, iv, iv - spread / 2.0, iv + spread / 2.0)
    }

    fn flat_smile(iv: f64) -> VolSmile {
        let config = VolSurfaceConfig::default();
        let points = vec![
            make_point(90.0, iv, 0.01),
            make_point(95.0, iv, 0.01),
            make_point(100.0, iv, 0.01),
            make_point(105.0, iv, 0.01),
            make_point(110.0, iv, 0.01),
        ];
        VolSmile::new(None, points, &config, 100.0)
    }

    fn skewed_smile() -> VolSmile {
        let config = VolSurfaceConfig::default();
        let points = vec![
            make_point(90.0, 0.35, 0.02),
            make_point(95.0, 0.28, 0.02),
            make_point(100.0, 0.25, 0.02),
            make_point(105.0, 0.27, 0.02),
            make_point(110.0, 0.30, 0.02),
        ];
        VolSmile::new(None, points, &config, 100.0)
    }

    #[test]
    fn call_spread_known_smile() {
        let smile = flat_smile(0.20);
        let result = call_spread_probability(100.0, &smile, 100.0, 1.0, 0.0, 10.0);

        let cs = result.expect("call spread should succeed for ATM strike with good smile");
        assert!(cs.probability > 0.3 && cs.probability < 0.7);
        assert!(cs.epsilon_used > 0.0);
        assert!(cs.k_lower < 100.0 && cs.k_upper > 100.0);
    }

    #[test]
    fn call_spread_none_when_epsilon_exceeds_max() {
        let smile = flat_smile(0.20);
        // Bracket half-width is 5.0; budget 1.0 forces failure.
        let result = call_spread_probability(100.0, &smile, 100.0, 1.0, 0.0, 1.0);
        assert!(result.is_none());
    }

    #[test]
    fn nd2_matches_call_spread_on_flat_surface() {
        let smile = flat_smile(0.20);
        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, 10.0).unwrap();

        let cs = extraction
            .call_spread
            .as_ref()
            .expect("call spread expected");
        let diff = (cs.probability - extraction.nd2.probability).abs();
        assert!(diff < 0.02);
    }

    #[test]
    fn nd2_skew_adjustment_correct() {
        let smile = skewed_smile();
        // ATM IV = 0.25 (at strike 100), strike 95 has IV = 0.28.
        let nd2 = nd2_probability(100.0, 95.0, 1.0, 0.28, smile.atm_iv);
        assert!((nd2.skew_adjustment - 0.03).abs() < 1e-10);

        let nd2_atm = nd2_probability(100.0, 100.0, 1.0, 0.25, smile.atm_iv);
        assert!(nd2_atm.skew_adjustment.abs() < 1e-10);
    }

    #[test]
    fn extract_uses_call_spread_as_primary() {
        let smile = flat_smile(0.20);
        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, 10.0).unwrap();

        assert_eq!(
            extraction.primary_method,
            ProbabilityMethod::CallSpreadReplication
        );
        assert!(extraction.call_spread.is_some());
    }

    #[test]
    fn extract_falls_back_to_nd2() {
        let smile = flat_smile(0.20);
        // Tiny epsilon budget forces call-spread to fail.
        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, 0.01).unwrap();

        assert_eq!(
            extraction.primary_method,
            ProbabilityMethod::Nd2SkewAdjusted
        );
        assert!(extraction.call_spread.is_none());
        assert!(extraction.method_disagreement.abs() < f64::EPSILON);
    }

    #[test]
    fn method_disagreement_on_skewed_surface() {
        let smile = skewed_smile();
        let extraction = extract_probabilities(95.0, &smile, 100.0, 1.0, 0.0, 10.0).unwrap();

        let cs = extraction
            .call_spread
            .as_ref()
            .expect("call spread expected");
        let expected = (cs.probability - extraction.nd2.probability).abs();
        assert!((extraction.method_disagreement - expected).abs() < 1e-10);
    }

    #[test]
    fn probability_clamped() {
        let smile = flat_smile(0.20);
        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, 0.0, 10.0).unwrap();
        assert!(extraction.primary_probability >= 0.0 && extraction.primary_probability <= 1.0);
        assert!(extraction.nd2.probability >= 0.0 && extraction.nd2.probability <= 1.0);
    }

    #[test]
    fn nd2_no_atm_iv_zero_skew() {
        let nd2 = nd2_probability(100.0, 100.0, 1.0, 0.20, None);
        assert!(nd2.skew_adjustment.abs() < f64::EPSILON);
    }

    /// HIGH-A-02 regression. Without the `/ df` rescaling the ATM call-spread
    /// probability at `r = 5%` would settle near `0.5 · exp(−0.05) ≈ 0.476`
    /// — visibly biased. After the fix it must remain near `0.5`.
    #[test]
    fn call_spread_with_nonzero_rate() {
        let smile = flat_smile(0.20);
        let rate = 0.05;

        let cs = call_spread_probability(100.0, &smile, 100.0, 1.0, rate, 10.0)
            .expect("call spread should succeed at non-zero rate");

        assert!(
            (cs.probability - 0.5).abs() < 0.05,
            "ATM call-spread probability at r=5% must be ~0.5, got {} \
             (an unrescaled implementation would give ~0.476)",
            cs.probability,
        );

        // Cross-check against N(d2): on a flat smile they must agree closely.
        let extraction = extract_probabilities(100.0, &smile, 100.0, 1.0, rate, 10.0).unwrap();
        let diff = (extraction.call_spread.unwrap().probability - extraction.nd2.probability).abs();
        assert!(
            diff < 0.02,
            "flat-surface methods must agree, got diff={diff}"
        );
    }

    /// Audit M-2: for an off-grid target the call spread must estimate
    /// `P(F > target)`, not `P(F > bracket-midpoint)`. On strikes
    /// {90,95,100,105,110}, target 96 brackets to (95,100) whose midpoint is
    /// 97.5 — the old code located the probability there.
    #[test]
    fn call_spread_recenters_on_off_grid_target() {
        let smile = flat_smile(0.20);
        let f = 100.0;
        let target = 96.0;
        let cs = call_spread_probability(target, &smile, f, 1.0, 0.0, 10.0)
            .expect("flat smile brackets the target");

        // True digital at the target vs at the (wrong) bracket midpoint.
        let p_target = nd2_probability(f, target, 1.0, 0.20, None).probability;
        let p_midpoint = nd2_probability(f, 97.5, 1.0, 0.20, None).probability;

        assert!(
            (cs.probability - p_target).abs() < 1.5e-2,
            "call spread {} should track P(F>{target})={p_target}",
            cs.probability,
        );
        assert!(
            (cs.probability - p_target).abs() < (cs.probability - p_midpoint).abs(),
            "call spread {} should be closer to the target digital {p_target} \
             than to the midpoint digital {p_midpoint}",
            cs.probability,
        );
        // The evaluated legs are centered on the target.
        assert!(((cs.k_lower + cs.k_upper) / 2.0 - target).abs() < 1e-9);
    }

    /// Audit M-2: an arbitraging smile (extreme wing IV makes the higher-strike
    /// call worth more than the lower-strike one) must yield `None`, not a
    /// silently clamped `0.0`.
    #[test]
    fn call_spread_rejects_arbitraging_smile() {
        let config = VolSurfaceConfig::default();
        let points = vec![
            make_point(90.0, 0.20, 0.01),
            make_point(95.0, 0.20, 0.01),
            make_point(100.0, 0.20, 0.01),
            make_point(105.0, 0.20, 0.01),
            make_point(110.0, 3.00, 0.01), // pathological wing
        ];
        let smile = VolSmile::new(None, points, &config, 100.0);
        let result = call_spread_probability(107.0, &smile, 100.0, 1.0, 0.0, 10.0);
        assert!(
            result.is_none(),
            "arbitraging smile must yield None, got {result:?}",
        );
    }

    /// At/after expiry the extractor returns `None` rather than leaking a NaN
    /// `nd2.probability` / `method_disagreement` (the `t <= 0` guard mirrors
    /// the intrinsic guards in `pricing`/`greeks`).
    #[test]
    fn extract_returns_none_at_or_after_expiry() {
        let smile = flat_smile(0.20);
        assert!(extract_probabilities(100.0, &smile, 100.0, 0.0, 0.0, 10.0).is_none());
        assert!(extract_probabilities(100.0, &smile, 100.0, -1.0, 0.0, 10.0).is_none());
    }

    /// For finite `t > 0` inputs no field of the extraction is ever NaN.
    #[test]
    fn extract_has_no_nan_fields_for_valid_inputs() {
        let smile = skewed_smile();
        for &k in &[95.0_f64, 100.0, 105.0] {
            let e = extract_probabilities(k, &smile, 100.0, 0.5, 0.03, 10.0).unwrap();
            assert!(e.primary_probability.is_finite());
            assert!(e.nd2.probability.is_finite());
            assert!(e.method_disagreement.is_finite());
            assert!(e.skew_adjustment.is_finite());
            if let Some(cs) = &e.call_spread {
                assert!(cs.probability.is_finite());
            }
        }
    }
}
