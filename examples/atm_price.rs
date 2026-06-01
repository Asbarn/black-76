//! Price an at-the-money call and put under Black-76.
//!
//! ```bash
//! cargo run --example atm_price
//! ```

use black_76::{call_price, put_price};

fn main() {
    let forward = 100.0_f64;
    let strike = 100.0_f64;
    let time_to_expiry = 1.0_f64;
    let sigma = 0.20_f64;
    let rate = 0.0_f64;

    let c = call_price(forward, strike, time_to_expiry, sigma, rate);
    let p = put_price(forward, strike, time_to_expiry, sigma, rate);

    println!("Black-76 ATM option pricing");
    println!("---------------------------");
    println!("F = {forward:.4}");
    println!("K = {strike:.4}");
    println!("T = {time_to_expiry:.4} years");
    println!("sigma = {sigma:.4}");
    println!("r = {rate:.4}");
    println!();
    println!("Call price: {c:.6}");
    println!("Put price:  {p:.6}");
    println!("C - P:      {:.6}", c - p);
    println!(
        "F - K:      {:.6}  (put-call parity at r=0)",
        forward - strike
    );
}
