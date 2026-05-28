//! Build a vol smile from observed (strike, IV) quotes, interpolate, and
//! report skew.
//!
//! Requires the `vol-surface` feature:
//!
//! ```bash
//! cargo run --example vol_smile --features vol-surface
//! ```

use black_76::vol_surface::{SmilePoint, VolSmile, VolSurfaceConfig};

fn main() {
    let config = VolSurfaceConfig::default();
    let forward = 100.0_f64;

    // Synthetic put-skew smile.
    let points = vec![
        SmilePoint::new(80.0, 0.36, 0.355, 0.365),
        SmilePoint::new(90.0, 0.30, 0.297, 0.303),
        SmilePoint::new(95.0, 0.27, 0.267, 0.273),
        SmilePoint::new(100.0, 0.25, 0.247, 0.253),
        SmilePoint::new(105.0, 0.26, 0.257, 0.263),
        SmilePoint::new(110.0, 0.28, 0.275, 0.285),
        SmilePoint::new(120.0, 0.32, 0.314, 0.326),
    ];

    let smile = VolSmile::new(None, points, &config, forward);

    println!(
        "Vol smile: F={forward}, {} usable strikes, quality = {:?}",
        smile.len(),
        smile.quality
    );
    println!("ATM IV = {:?}", smile.atm_iv);
    println!();
    println!("{:>10} {:>10} {:>10}", "Strike", "IV", "Skew");
    for k in [85.0_f64, 92.5, 97.5, 100.0, 102.5, 107.5, 115.0] {
        let iv = smile.interpolate(k).unwrap();
        let skew = smile.skew_at(k).unwrap();
        println!("{k:>10.2} {iv:>10.4} {skew:>+10.4}");
    }

    println!();
    if let Some((kl, ku)) = smile.nearest_bracket(101.0) {
        println!("Nearest bracket around 101.0: ({kl:.2}, {ku:.2})");
    }
}
