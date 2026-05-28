//! Black-76 option pricing model: closed-form prices, Greeks, and implied
//! volatility solver for futures and forward options.
//!
//! The Black-76 model (Fischer Black, *The Pricing of Commodity Contracts*,
//! Journal of Financial Economics, 1976) prices European options on forward
//! and futures contracts. Use it for Eurodollar options, FX forward options,
//! commodity-futures options, and crypto perpetual-futures options where
//! the underlying is a forward, not a spot.
//!
//! # Quick start
//!
//! ```
//! use black_76::{call_price, solve_iv, SolverConfig};
//!
//! // Price an ATM call: F=100, K=100, T=1 year, sigma=20%, r=0
//! let c = call_price(100.0, 100.0, 1.0, 0.20, 0.0);
//! assert!((c - 7.9656).abs() < 1e-3);
//!
//! // Solve for implied volatility given a market price
//! let cfg = SolverConfig::default();
//! let result = solve_iv(7.9656, 100.0, 100.0, 1.0, 0.0, true, &cfg);
//! assert!(result.converged);
//! assert!((result.iv - 0.20).abs() < 1e-4);
//! ```
//!
//! # Modules
//!
//! - [`pricing`] — closed-form Black-76 call/put prices, vega, intrinsic value.
//! - [`iv_solver`] — Newton-Raphson with Brent's-method fallback.
//! - [`greeks`] — analytical first-order Greeks (delta, vega, theta).
//! - [`types`] — `OptionType`, `SolverMethod`, `SolverResult`, `InstrumentGreeks`.
//! - [`config`] — `SolverConfig` (with builder) for tuning solver parameters.
//! - [`vol_surface`] (feature `vol-surface`) — per-expiry IV smile with linear interpolation.
//! - [`digital`] (feature `digital`) — risk-neutral probability extraction via call-spread replication and N(d2).
//!
//! # Feature flags
//!
//! - `serde` — adds `Serialize` / `Deserialize` derives on public types.
//! - `vol-surface` — enables [`vol_surface`].
//! - `digital` — enables [`digital`] (requires `vol-surface`).
//!
//! # Convergence checking
//!
//! [`iv_solver::solve_iv`] returns a [`SolverResult`] whose `converged: bool`
//! field MUST be checked before consuming `iv`. When the market price is
//! outside the feasible Black-76 range (e.g., below intrinsic or above
//! `F·exp(-rT)`), `iv` is `f64::NAN` and `converged` is `false`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

pub mod config;
pub mod greeks;
pub mod iv_solver;
pub mod pricing;
pub mod types;

#[cfg(feature = "vol-surface")]
#[cfg_attr(docsrs, doc(cfg(feature = "vol-surface")))]
pub mod vol_surface;

#[cfg(feature = "digital")]
#[cfg_attr(docsrs, doc(cfg(feature = "digital")))]
pub mod digital;

// Curated top-level re-exports
pub use config::SolverConfig;
pub use greeks::compute_greeks;
pub use iv_solver::{solve_iv, solve_iv_triple};
pub use pricing::{call_price, d1_d2, intrinsic_value, price, put_price, vega};
pub use types::{InstrumentGreeks, OptionType, SolverMethod, SolverResult};
