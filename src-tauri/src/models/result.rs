use serde::{Deserialize, Serialize};

use super::trade::TradeResult;

/// A point on the equity curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: String,
    pub equity: f64,
}

/// A point on the drawdown curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawdownPoint {
    pub timestamp: String,
    pub drawdown_pct: f64,
}

/// All performance metrics from a backtest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestMetrics {
    // Returns
    pub final_capital: f64,
    pub total_return_pct: f64,
    pub annualized_return_pct: f64,
    pub monthly_return_avg_pct: f64,

    // Risk-adjusted
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,

    // Drawdown
    pub max_drawdown_pct: f64,
    pub max_drawdown_duration_bars: usize,
    pub max_drawdown_duration_time: String,
    pub avg_drawdown_pct: f64,
    pub recovery_factor: f64,

    // Trades
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub breakeven_trades: usize,
    pub win_rate_pct: f64,

    // P&L
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub net_profit: f64,
    pub profit_factor: f64,
    pub avg_trade: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub expectancy: f64,

    // Consistency
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,
    pub avg_consecutive_wins: f64,
    pub avg_consecutive_losses: f64,

    // Time
    pub avg_trade_duration: String,
    pub avg_bars_in_trade: f64,
    pub avg_winner_duration: String,
    pub avg_loser_duration: String,

    // Risk
    pub mae_avg: f64,
    pub mae_max: f64,
    pub mfe_avg: f64,
    pub mfe_max: f64,
}

/// Complete results of a backtest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub trades: Vec<TradeResult>,
    pub equity_curve: Vec<EquityPoint>,
    pub drawdown_curve: Vec<DrawdownPoint>,
    pub returns: Vec<f64>,
    pub metrics: BacktestMetrics,
}
