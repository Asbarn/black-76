//! Extract a risk-neutral probability `P_Q(F_T > K)` using both
//! call-spread replication and `N(d2)`, and report their disagreement.
//!
//! Requires the `digital` feature (which transitively enables `vol-surface`):
//!
//! ```bash
//! cargo run --example digital_prob --features digital
//! ```

use black_76::digital::{ProbabilityMethod, extract_probabilities};
use black_76::vol_surface::{SmilePoint, VolSmile, VolSurfaceConfig};

fn main() {
    let config = VolSurfaceConfig::default();
    let forward = 100.0_f64;
    let time_to_expiry = 0.50_f64;
    let rate = 0.04_f64;
    let max_epsilon = 5.0_f64;

    let points = vec![
        SmilePoint::new(85.0, 0.34, 0.335, 0.345),
        SmilePoint::new(90.0, 0.30, 0.297, 0.303),
        SmilePoint::new(95.0, 0.27, 0.267, 0.273),
        SmilePoint::new(100.0, 0.25, 0.247, 0.253),
        SmilePoint::new(105.0, 0.26, 0.257, 0.263),
        SmilePoint::new(110.0, 0.28, 0.275, 0.285),
        SmilePoint::new(115.0, 0.30, 0.295, 0.305),
    ];
    let smile = VolSmile::new(None, points, &config, forward);

    println!("Risk-neutral probability extraction");
    println!("F = {forward}  T = {time_to_expiry}  r = {rate}  max_eps = {max_epsilon}");
    println!("------------------------------------------------------------------");
    println!(
        "{:>8} {:>10} {:>10} {:>12} {:>10}",
        "Strike", "P (primary)", "Method", "Disagreement", "Skew adj"
    );

    for k in [90.0_f64, 95.0, 100.0, 105.0, 110.0] {
        let extraction =
            extract_probabilities(k, &smile, forward, time_to_expiry, rate, max_epsilon)
                .expect("smile interpolates at all evaluated strikes");

        let method_tag = match extraction.primary_method {
            ProbabilityMethod::CallSpreadReplication => "CallSpread",
            ProbabilityMethod::Nd2SkewAdjusted => "N(d2)",
            // `ProbabilityMethod` is #[non_exhaustive]; absorb future variants.
            _ => "Other",
        };

        println!(
            "{:>8.2} {:>10.4} {:>10} {:>12.4} {:>+10.4}",
            k,
            extraction.primary_probability,
            method_tag,
            extraction.method_disagreement,
            extraction.skew_adjustment,
        );
    }
}
