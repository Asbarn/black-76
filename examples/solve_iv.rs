//! Solve for implied volatility given a market price.
//!
//! Demonstrates the convergence-check contract: always inspect
//! [`SolverResult::converged`](black_76::SolverResult) before consuming `iv`.
//!
//! ```bash
//! cargo run --example solve_iv
//! ```

use black_76::{SolverConfig, call_price, solve_iv};

fn main() {
    let forward = 100.0_f64;
    let strike = 100.0_f64;
    let time_to_expiry = 1.0_f64;
    let rate = 0.05_f64;
    let true_sigma = 0.25_f64;

    // Synthesize a market price from a known sigma.
    let market_price = call_price(forward, strike, time_to_expiry, true_sigma, rate);
    println!("Synthesised market price: {market_price:.6} (true sigma = {true_sigma:.4})");
    println!();

    let config = SolverConfig::default();
    let result = solve_iv(
        market_price,
        forward,
        strike,
        time_to_expiry,
        rate,
        true,
        &config,
    );

    if result.converged {
        println!(
            "Converged via {:?} in {} iteration(s):",
            result.method, result.iterations
        );
        println!("  solved sigma = {:.10}", result.iv);
        println!("  true sigma   = {true_sigma:.10}");
        println!("  sigma error  = {:.2e}", (result.iv - true_sigma).abs());
        println!("  residual  = {:.2e}", result.residual);
    } else {
        eprintln!("Solver did NOT converge; do NOT consume iv (it may be NaN).");
        eprintln!("Result: {result:?}");
        std::process::exit(1);
    }

    // Show the contract under an infeasible price: too high.
    let bad = solve_iv(
        1_000.0,
        forward,
        strike,
        time_to_expiry,
        rate,
        true,
        &config,
    );
    println!();
    println!("Infeasible price example (market = 1000):");
    println!("  converged = {}", bad.converged);
    println!("  iv        = {} (NaN expected)", bad.iv);
}
