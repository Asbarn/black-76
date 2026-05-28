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
    pub fn is_call(self) -> bool {
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

/// Result of an [`iv_solver::solve_iv`](crate::iv_solver::solve_iv) attempt.
///
/// **Important:** check `converged` before consuming `iv`. When no root
/// exists in the feasible IV range (market price below intrinsic, above
/// `F·exp(-rT)`, or otherwise outside the Black-76 envelope), `iv` is
/// `f64::NAN` and `converged` is `false`.
///
/// New fields may be added in future minor versions; construct via the
/// solver, not via field initializers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct SolverResult {
    /// Solved implied volatility (annualized). `f64::NAN` if not converged.
    pub iv: f64,
    /// Solver method used to produce this result.
    pub method: SolverMethod,
    /// Number of iterations taken.
    pub iterations: u32,
    /// Whether the solver converged within the configured tolerance.
    pub converged: bool,
    /// Residual `|model_price - market_price|` at the solution (or at the
    /// best endpoint when no root was found).
    pub residual: f64,
}

// ---------------------------------------------------------------------------
// Greeks
// ---------------------------------------------------------------------------

/// First-order Greeks for a single option.
///
/// Gamma is intentionally omitted; if you need it, use the finite-difference
/// pattern in `examples/greeks.rs` or open an issue.
///
/// Sign and unit conventions:
/// - **delta**: dimensionless; positive for calls, negative for puts.
/// - **vega**: price change per 1% absolute change in volatility (i.e.,
///   `vega_per_001 = raw_dv_dsigma / 100`).
/// - **theta**: per-day time decay (per calendar day; division by 365.25).
///
/// New fields may be added in future minor versions.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct InstrumentGreeks {
    /// Delta: sensitivity to the forward, `df · N(d1)` (call) or `df · (N(d1) - 1)` (put).
    pub delta: f64,
    /// Vega: price change for a 1% absolute change in IV (per-1%, not per-1).
    pub vega: f64,
    /// Theta: per-day price decay.
    pub theta: f64,
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
