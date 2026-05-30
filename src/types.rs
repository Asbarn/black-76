//! Core types: `OptionType`, `SolverMethod`, `SolverResult`, `InstrumentGreeks`.
//!
//! All numerical math uses `f64`. The crate exposes no `Decimal`-typed APIs.

/// Option type: call or put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OptionType {
    /// European call option (right to buy at strike).
    Call,
    /// European put option (right to sell at strike).
    Put,
}

impl OptionType {
    /// Returns `true` if this is a call.
    #[inline]
    #[must_use]
    pub const fn is_call(self) -> bool {
        matches!(self, OptionType::Call)
    }
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptionType::Call => write!(f, "Call"),
            OptionType::Put => write!(f, "Put"),
        }
    }
}

// ---------------------------------------------------------------------------
// IV Solver types
// ---------------------------------------------------------------------------

/// Method used by the IV solver.
///
/// New variants may be added in future minor versions; match exhaustively
/// at your own peril (the enum is `#[non_exhaustive]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SolverMethod {
    /// Newton-Raphson iteration.
    NewtonRaphson,
    /// Brent's method (bracketed root-finding).
    Brent,
}

/// Why the IV solver returned — the precise outcome behind a (non-)convergence
/// (see [`SolverResult`]).
///
/// New variants may be added in future minor versions (the enum is
/// `#[non_exhaustive]`); always include a wildcard arm when matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SolverStatus {
    /// A volatility was found within tolerance. `iv` is finite and usable.
    Converged,
    /// Time to expiry was below the near-expiry cutoff; the option should be
    /// priced intrinsically and no implied volatility is solved. `iv` is
    /// [`f64::NAN`].
    NearExpiryIntrinsic,
    /// The market price was zero or negative — no implied volatility exists.
    /// `iv` is [`f64::NAN`].
    NonPositivePrice,
    /// The market price was below the discounted intrinsic value (the
    /// Black-76 no-arbitrage lower bound). `iv` is [`f64::NAN`].
    BelowIntrinsic,
    /// No root exists in `[iv_min, iv_max]` — the price is unattainable for
    /// any volatility in that range. `iv` is [`f64::NAN`].
    NoBracketInRange,
    /// Vega is below `vega_floor`, so the implied volatility is not
    /// numerically identifiable from the price. `iv` is [`f64::NAN`].
    NotIdentifiable,
    /// The solver exhausted its iteration budget without meeting the
    /// volatility-space tolerance. `iv` is [`f64::NAN`].
    MaxIterations,
}

/// Result of an [`iv_solver::solve_iv`](crate::iv_solver::solve_iv) attempt.
///
/// **Check `converged` (or `status`) before consuming `iv`.** Whenever the
/// solver does not converge, `iv` is [`f64::NAN`] and `status` explains why.
/// `converged` is exactly `status == SolverStatus::Converged`.
///
/// New fields may be added in future minor versions; construct via the
/// solver, not via field initializers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct SolverResult {
    /// Solved implied volatility (annualized). [`f64::NAN`] unless `status` is
    /// [`SolverStatus::Converged`].
    pub iv: f64,
    /// Solver method used to produce this result.
    pub method: SolverMethod,
    /// Number of iterations taken.
    pub iterations: u32,
    /// Whether the solver converged. Equivalent to
    /// `status == SolverStatus::Converged`.
    pub converged: bool,
    /// The precise outcome of the solve (the reason behind `converged`).
    pub status: SolverStatus,
    /// Residual `|model_price - market_price|` at the solution (or at the
    /// best endpoint examined when no root was found).
    pub residual: f64,
}

// ---------------------------------------------------------------------------
// Greeks
// ---------------------------------------------------------------------------

/// First-order Greeks (plus gamma) for a single option.
///
/// Sign and unit conventions:
/// - **delta**: dimensionless; `df · N(d1)` (call) or `df · (N(d1) − 1)` (put).
/// - **gamma**: `∂²price/∂F²`, per unit forward (raw, not scaled); identical
///   for calls and puts.
/// - **vega**: price change per **1%** absolute change in volatility
///   (`raw_dv_dsigma / 100`); identical for calls and puts.
/// - **theta**: per-calendar-day time decay (year = 365.25 days); negative
///   for long positions.
/// - **rho**: price change per **1%** absolute change in the rate
///   (`∂price/∂r / 100`); negative under Black-76 (`∂C/∂r = −T·C`).
///
/// New fields may be added in future minor versions.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct InstrumentGreeks {
    /// Delta: sensitivity to the forward, `df · N(d1)` (call) or `df · (N(d1) − 1)` (put).
    pub delta: f64,
    /// Gamma: `∂²price/∂F² = df · n(d1) / (F · σ · √T)` (per unit forward, raw).
    pub gamma: f64,
    /// Vega: price change for a 1% absolute change in IV (per-1%, not per-1).
    pub vega: f64,
    /// Theta: per-day price decay.
    pub theta: f64,
    /// Rho: price change for a 1% absolute change in the rate (per-1%).
    pub rho: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_type_display() {
        assert_eq!(format!("{}", OptionType::Call), "Call");
        assert_eq!(format!("{}", OptionType::Put), "Put");
    }

    #[test]
    fn option_type_is_call() {
        assert!(OptionType::Call.is_call());
        assert!(!OptionType::Put.is_call());
    }
}
