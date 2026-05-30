//! Reference-value cross-check for Black-76 pricing.
//!
//! Two kinds of expected value appear in [`CASES`]:
//!
//! - **External reference** — the row's expected value comes from a
//!   published source (Hull's textbook). A drift here means the math is
//!   wrong, not just changed.
//! - **Regression lock** — the row's expected value was computed once by
//!   this library at v0.1.0 and committed verbatim. A drift here means
//!   *something changed* between releases; investigate before bumping the
//!   tolerance.
//!
//! Put prices are cross-checked via put-call parity (`P = C − exp(−rT) ·
//! (F − K)`). Parity is a structural identity, so a put-side drift caught
//! by [`golden_put_prices_via_parity`] points to a regression in the put
//! pricing path independently of the call values above.

use black_76::{call_price, put_price};

/// `(F, K, T, σ, r, expected_C, kind)`. `kind` is documentation-only.
const CASES: &[(f64, f64, f64, f64, f64, f64, &str)] = &[
    // Hull, *Options, Futures, and Other Derivatives*, 10th ed., §17.6 — the
    // canonical ATM Black-76 example.
    (
        100.0,
        100.0,
        1.0,
        0.20,
        0.00,
        7.965_567_455_405_804,
        "external: Hull §17.6 ATM, r=0",
    ),
    // Same point with r = 5%; cross-validated by C(r=r) = C(r=0) · exp(−rT)
    // at F = K (rate enters only through df in Black-76).
    (
        100.0,
        100.0,
        1.0,
        0.20,
        0.05,
        7.577_082_146_427_28,
        "external (parity to row 1)",
    ),
    // OTM call, regression lock.
    (
        100.0,
        120.0,
        0.5,
        0.30,
        0.03,
        2.466_498_852_689_267,
        "regression lock",
    ),
    // ITM call, regression lock.
    (
        110.0,
        100.0,
        0.25,
        0.40,
        0.02,
        14.220_729_239_461_19,
        "regression lock",
    ),
    // Long-dated, high-vol regime, regression lock.
    (
        50.0,
        55.0,
        2.0,
        0.50,
        0.01,
        11.892_954_840_563_526,
        "regression lock",
    ),
    // External references computed with an independent `math.erf` implementation
    // (Python, not this crate's `statrs`): a drift here means the math is wrong,
    // not merely changed.
    (
        50.0,
        48.0,
        0.75,
        0.35,
        0.02,
        6.852_097_035_414_271,
        "external: independent erf reference",
    ),
    (
        4200.0,
        4000.0,
        0.30,
        0.45,
        0.015,
        507.654_133_607_437,
        "external: independent erf reference (crypto-scale forward)",
    ),
    (
        1.0,
        1.2,
        2.0,
        0.60,
        0.0,
        0.269_288_257_583_031_54,
        "external: independent erf reference",
    ),
];

#[test]
fn golden_call_prices() {
    for &(f, k, t, sigma, r, expected, kind) in CASES {
        let actual = call_price(f, k, t, sigma, r);
        let err = (actual - expected).abs();
        // Scale-relative tolerance so large-forward (crypto-scale) rows are not
        // held to a sub-ULP absolute bound while small values stay tight.
        let bound = 1e-12 * (1.0 + expected.abs());
        assert!(
            err < bound,
            "{kind}: F={f}, K={k}, T={t}, σ={sigma}, r={r} — expected {expected}, got {actual}, |err|={err:.2e}",
        );
    }
}

#[test]
fn golden_put_prices_via_parity() {
    // Verify the put pricing path through `P = C − df·(F − K)`. Catches an
    // independent regression in the put branch of `pricing.rs` even if the
    // call branch matches its golden value above.
    for &(f, k, t, sigma, r, _expected_c, kind) in CASES {
        let actual_c = call_price(f, k, t, sigma, r);
        let actual_p = put_price(f, k, t, sigma, r);
        let df = (-r * t).exp();
        let parity_p = actual_c - df * (f - k);
        let err = (actual_p - parity_p).abs();
        let bound = 1e-12 * (1.0 + f.abs());
        assert!(
            err < bound,
            "{kind}: put violates parity — P={actual_p}, parity_P={parity_p}, |err|={err:.2e}",
        );
    }
}
