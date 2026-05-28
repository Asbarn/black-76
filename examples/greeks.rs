//! Compute analytical first-order Greeks and cross-check vega against a
//! finite difference.
//!
//! ```bash
//! cargo run --example greeks
//! ```

use black_76::{call_price, compute_greeks};

fn main() {
    let forward = 100.0_f64;
    let strike = 100.0_f64;
    let time_to_expiry = 1.0_f64;
    let sigma = 0.20_f64;
    let rate = 0.05_f64;

    let call = compute_greeks(forward, strike, time_to_expiry, sigma, rate, true);
    let put = compute_greeks(forward, strike, time_to_expiry, sigma, rate, false);

    println!("Greeks at F={forward} K={strike} T={time_to_expiry} σ={sigma} r={rate}");
    println!("-----------------------------------------------------------");
    println!("            {:>14} {:>14}", "Call", "Put");
    println!("delta       {:>14.6} {:>14.6}", call.delta, put.delta);
    println!("vega (1%)   {:>14.6} {:>14.6}", call.vega, put.vega);
    println!("theta /day  {:>14.6} {:>14.6}", call.theta, put.theta);

    // Finite-difference cross-check of vega (per-unit sigma, then per 1%).
    let bump = 1e-4_f64;
    let c_up = call_price(forward, strike, time_to_expiry, sigma + bump, rate);
    let c_dn = call_price(forward, strike, time_to_expiry, sigma - bump, rate);
    let fd_vega_per_unit = (c_up - c_dn) / (2.0 * bump);
    let fd_vega_per_1pct = fd_vega_per_unit / 100.0;

    println!();
    println!("Finite-difference vega cross-check (call, bump={bump:.1e}):");
    println!("  analytic vega (per 1%) = {:.10}", call.vega);
    println!("  FD vega       (per 1%) = {fd_vega_per_1pct:.10}");
    println!(
        "  abs error              = {:.2e}",
        (call.vega - fd_vega_per_1pct).abs()
    );
}
