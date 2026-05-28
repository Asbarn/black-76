//! Verify Black-76 put-call parity across a strike grid:
//!
//! ```text
//! C − P = exp(−rT) · (F − K)
//! ```
//!
//! ```bash
//! cargo run --example put_call_parity
//! ```

use black_76::{call_price, put_price};

fn main() {
    let forward = 100.0_f64;
    let time_to_expiry = 0.5_f64;
    let sigma = 0.30_f64;
    let rate = 0.04_f64;
    let df = (-rate * time_to_expiry).exp();

    println!("Black-76 put-call parity check");
    println!("F = {forward:.2}  T = {time_to_expiry:.2}  σ = {sigma:.2}  r = {rate:.2}");
    println!("---------------------------------------------------------------");
    println!(
        "{:>10} {:>14} {:>14} {:>14} {:>14}",
        "K", "C", "P", "C − P", "df · (F − K)"
    );

    let strikes = [70.0_f64, 85.0, 95.0, 100.0, 105.0, 115.0, 130.0];
    let mut max_err = 0.0_f64;

    for k in strikes {
        let c = call_price(forward, k, time_to_expiry, sigma, rate);
        let p = put_price(forward, k, time_to_expiry, sigma, rate);
        let lhs = c - p;
        let rhs = df * (forward - k);
        let err = (lhs - rhs).abs();
        max_err = max_err.max(err);
        println!("{k:>10.2} {c:>14.6} {p:>14.6} {lhs:>14.6} {rhs:>14.6}");
    }

    println!();
    println!("Max parity residual: {max_err:.2e}");
    assert!(max_err < 1e-10, "parity must hold to machine precision");
}
