use crate::models::config::Timeframe;
use crate::models::result::{BacktestMetrics, EquityPoint};
use crate::models::trade::TradeResult;

/// Calculate the number of bars per trading day for a given timeframe.
/// Used for annualizing returns and risk-adjusted ratios.
fn bars_per_day(tf: Timeframe) -> f64 {
    match tf {
        Timeframe::Tick => 1440.0, // Treat like M1 (approximation)
        Timeframe::M1 => 1440.0,   // 24h * 60
        Timeframe::M5 => 288.0,    // 24h * 12
        Timeframe::M15 => 96.0,    // 24h * 4
        Timeframe::M30 => 48.0,    // 24h * 2
        Timeframe::H1 => 24.0,
        Timeframe::H4 => 6.0,
        Timeframe::D1 => 1.0,
    }
}

/// Calculate all backtest metrics from trades and equity curve.
pub fn calculate_metrics(
    trades: &[TradeResult],
    equity_curve: &[EquityPoint],
    initial_capital: f64,
    timeframe: Timeframe,
) -> BacktestMetrics {
    let total_trades = trades.len();

    if total_trades == 0 {
        return empty_metrics(initial_capital);
    }

    // ── Trade classification ──
    let winning: Vec<&TradeResult> = trades.iter().filter(|t| t.pnl > 0.0).collect();
    let losing: Vec<&TradeResult> = trades.iter().filter(|t| t.pnl < 0.0).collect();
    let breakeven: Vec<&TradeResult> = trades.iter().filter(|t| t.pnl == 0.0).collect();

    let winning_trades = winning.len();
    let losing_trades = losing.len();
    let breakeven_trades = breakeven.len();
    let win_rate_pct = winning_trades as f64 / total_trades as f64 * 100.0;

    // ── P&L ──
    let gross_profit: f64 = winning.iter().map(|t| t.pnl).sum();
    let gross_loss: f64 = losing.iter().map(|t| t.pnl.abs()).sum();
    let total_commission: f64 = trades.iter().map(|t| t.commission).sum();
    let net_profit: f64 = trades.iter().map(|t| t.pnl).sum::<f64>() - total_commission;
    let profit_factor = if gross_loss > 0.0 {
        gross_profit / gross_loss
    } else if gross_profit > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    let avg_trade = trades.iter().map(|t| t.pnl).sum::<f64>() / total_trades as f64;
    let avg_win = if winning_trades > 0 {
        gross_profit / winning_trades as f64
    } else {
        0.0
    };
    let avg_loss = if losing_trades > 0 {
        -gross_loss / losing_trades as f64
    } else {
        0.0
    };
    let largest_win = winning.iter().map(|t| t.pnl).fold(0.0f64, f64::max);
    let largest_loss = losing.iter().map(|t| t.pnl).fold(0.0f64, f64::min);
    let expectancy = if total_trades > 0 {
        (win_rate_pct / 100.0) * avg_win + (1.0 - win_rate_pct / 100.0) * avg_loss
    } else {
        0.0
    };

    // ── Returns ──
    let final_capital = initial_capital + net_profit;
    let total_return_pct = net_profit / initial_capital * 100.0;

    // Annualized return: estimate trading days from equity curve using actual timeframe
    let trading_bars = equity_curve.len().max(1);
    let bpd = bars_per_day(timeframe);
    let annualized_return_pct = annualize_return(total_return_pct, trading_bars, bpd);
    let bars_per_month = bpd * 21.0; // ~21 trading days per month
    let monthly_return_avg_pct = if trading_bars > 0 {
        total_return_pct / (trading_bars as f64 / bars_per_month).max(1.0)
    } else {
        0.0
    };

    // ── Drawdown ──
    let (max_drawdown_pct, max_dd_duration_bars, avg_drawdown_pct) =
        calculate_drawdown_stats(equity_curve);
    let recovery_factor = if max_drawdown_pct > 0.0 {
        net_profit / (initial_capital * max_drawdown_pct / 100.0)
    } else {
        0.0
    };

    // ── Risk-adjusted ──
    // Annualization factor: number of trades we'd expect per year
    // For per-trade returns, use 252 trading days as the annualization basis
    let annualization_factor = 252.0;
    let trade_returns: Vec<f64> = trades.iter().map(|t| t.pnl / initial_capital).collect();
    let sharpe_ratio = calculate_sharpe(&trade_returns, annualization_factor);
    let sortino_ratio = calculate_sortino(&trade_returns, annualization_factor);
    let calmar_ratio = if max_drawdown_pct > 0.0 {
        annualized_return_pct / max_drawdown_pct
    } else {
        0.0
    };

    // ── Consistency ──
    let (max_consec_wins, max_consec_losses, avg_consec_wins, avg_consec_losses) =
        calculate_consecutive(trades);

    // ── Time ──
    let mpb = timeframe.minutes().max(1); // minutes per bar (min 1 for tick)
    let avg_bars_in_trade =
        trades.iter().map(|t| t.duration_bars).sum::<usize>() as f64 / total_trades as f64;
    let avg_trade_duration = format_bars(avg_bars_in_trade as usize, mpb);

    let avg_winner_bars = if winning_trades > 0 {
        winning.iter().map(|t| t.duration_bars).sum::<usize>() as f64 / winning_trades as f64
    } else {
        0.0
    };
    let avg_loser_bars = if losing_trades > 0 {
        losing.iter().map(|t| t.duration_bars).sum::<usize>() as f64 / losing_trades as f64
    } else {
        0.0
    };

    // ── Risk (MAE/MFE) ──
    let mae_avg = if total_trades > 0 {
        trades.iter().map(|t| t.mae).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };
    let mae_max = trades.iter().map(|t| t.mae).fold(0.0f64, f64::max);
    let mfe_avg = if total_trades > 0 {
        trades.iter().map(|t| t.mfe).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };
    let mfe_max = trades.iter().map(|t| t.mfe).fold(0.0f64, f64::max);

    // ── Stagnation (longest period without new equity high) ──
    let stagnation_bars = calculate_stagnation_bars(equity_curve);
    let stagnation_time = format_bars(stagnation_bars, mpb);

    // ── Ulcer Index % ──
    let ulcer_index_pct = calculate_ulcer_index(equity_curve);

    BacktestMetrics {
        final_capital,
        total_return_pct,
        annualized_return_pct,
        monthly_return_avg_pct,
        sharpe_ratio,
        sortino_ratio,
        calmar_ratio,
        max_drawdown_pct,
        max_drawdown_duration_bars: max_dd_duration_bars,
        max_drawdown_duration_time: format_bars(max_dd_duration_bars, mpb),
        avg_drawdown_pct,
        recovery_factor,
        total_trades,
        winning_trades,
        losing_trades,
        breakeven_trades,
        win_rate_pct,
        gross_profit,
        gross_loss,
        net_profit,
        profit_factor,
        avg_trade,
        avg_win,
        avg_loss,
        largest_win,
        largest_loss,
        expectancy,
        max_consecutive_wins: max_consec_wins,
        max_consecutive_losses: max_consec_losses,
        avg_consecutive_wins: avg_consec_wins,
        avg_consecutive_losses: avg_consec_losses,
        avg_trade_duration,
        avg_bars_in_trade,
        avg_winner_duration: format_bars(avg_winner_bars as usize, mpb),
        avg_loser_duration: format_bars(avg_loser_bars as usize, mpb),
        mae_avg,
        mae_max,
        mfe_avg,
        mfe_max,
        stagnation_bars,
        stagnation_time,
        ulcer_index_pct,
        return_dd_ratio: if max_drawdown_pct > 0.0 {
            total_return_pct / max_drawdown_pct
        } else if total_return_pct > 0.0 {
            f64::INFINITY
        } else {
            0.0
        },
    }
}

/// Return default metrics for zero-trade case.
fn empty_metrics(initial_capital: f64) -> BacktestMetrics {
    BacktestMetrics {
        final_capital: initial_capital,
        total_return_pct: 0.0,
        annualized_return_pct: 0.0,
        monthly_return_avg_pct: 0.0,
        sharpe_ratio: 0.0,
        sortino_ratio: 0.0,
        calmar_ratio: 0.0,
        max_drawdown_pct: 0.0,
        max_drawdown_duration_bars: 0,
        max_drawdown_duration_time: "0m".to_string(),
        avg_drawdown_pct: 0.0,
        recovery_factor: 0.0,
        total_trades: 0,
        winning_trades: 0,
        losing_trades: 0,
        breakeven_trades: 0,
        win_rate_pct: 0.0,
        gross_profit: 0.0,
        gross_loss: 0.0,
        net_profit: 0.0,
        profit_factor: 0.0,
        avg_trade: 0.0,
        avg_win: 0.0,
        avg_loss: 0.0,
        largest_win: 0.0,
        largest_loss: 0.0,
        expectancy: 0.0,
        max_consecutive_wins: 0,
        max_consecutive_losses: 0,
        avg_consecutive_wins: 0.0,
        avg_consecutive_losses: 0.0,
        avg_trade_duration: "0m".to_string(),
        avg_bars_in_trade: 0.0,
        avg_winner_duration: "0m".to_string(),
        avg_loser_duration: "0m".to_string(),
        mae_avg: 0.0,
        mae_max: 0.0,
        mfe_avg: 0.0,
        mfe_max: 0.0,
        stagnation_bars: 0,
        stagnation_time: "0m".to_string(),
        ulcer_index_pct: 0.0,
        return_dd_ratio: 0.0,
    }
}

/// Annualize a return percentage based on bars and bars-per-day for the timeframe.
fn annualize_return(total_return_pct: f64, bars: usize, bpd: f64) -> f64 {
    if bars == 0 {
        return 0.0;
    }
    let bars_per_year = 252.0 * bpd;
    let years = bars as f64 / bars_per_year;
    if years <= 0.0 {
        return total_return_pct;
    }
    let total_factor = 1.0 + total_return_pct / 100.0;
    if total_factor <= 0.0 {
        return -100.0;
    }
    (total_factor.powf(1.0 / years) - 1.0) * 100.0
}

/// Calculate drawdown statistics from the equity curve.
fn calculate_drawdown_stats(equity_curve: &[EquityPoint]) -> (f64, usize, f64) {
    if equity_curve.is_empty() {
        return (0.0, 0, 0.0);
    }

    let mut peak = equity_curve[0].equity;
    let mut max_dd_pct = 0.0f64;
    let mut current_dd_start = 0usize;
    let mut max_dd_duration = 0usize;
    let mut dd_sum = 0.0f64;
    let mut dd_count = 0usize;

    for (i, point) in equity_curve.iter().enumerate() {
        if point.equity > peak {
            peak = point.equity;
            current_dd_start = i;
        }

        let dd_pct = if peak > 0.0 {
            (peak - point.equity) / peak * 100.0
        } else {
            0.0
        };

        if dd_pct > max_dd_pct {
            max_dd_pct = dd_pct;
            max_dd_duration = i - current_dd_start;
        }

        if dd_pct > 0.0 {
            dd_sum += dd_pct;
            dd_count += 1;
        }
    }

    let avg_dd = if dd_count > 0 {
        dd_sum / dd_count as f64
    } else {
        0.0
    };

    (max_dd_pct, max_dd_duration, avg_dd)
}

/// Sharpe Ratio: mean(returns) / std(returns) * sqrt(annualization_factor).
fn calculate_sharpe(returns: &[f64], annualization_factor: f64) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return 0.0;
    }
    (mean / std_dev) * annualization_factor.sqrt()
}

/// Sortino Ratio: mean(returns) / downside_deviation * sqrt(annualization_factor).
fn calculate_sortino(returns: &[f64], annualization_factor: f64) -> f64 {
    let n = returns.len();
    if n < 2 {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / n as f64;
    let negative_returns: Vec<f64> = returns.iter().filter(|&&r| r < 0.0).copied().collect();
    let neg_count = negative_returns.len();
    if neg_count == 0 {
        return 0.0; // No downside → can't compute meaningful Sortino
    }
    let downside_sum: f64 = negative_returns.iter().map(|r| r.powi(2)).sum();
    let downside_dev = (downside_sum / neg_count as f64).sqrt();
    if downside_dev == 0.0 {
        return 0.0;
    }
    (mean / downside_dev) * annualization_factor.sqrt()
}

/// Calculate consecutive wins/losses stats.
fn calculate_consecutive(trades: &[TradeResult]) -> (usize, usize, f64, f64) {
    if trades.is_empty() {
        return (0, 0, 0.0, 0.0);
    }

    let mut max_wins = 0usize;
    let mut max_losses = 0usize;
    let mut current_wins = 0usize;
    let mut current_losses = 0usize;
    let mut win_streaks: Vec<usize> = Vec::new();
    let mut loss_streaks: Vec<usize> = Vec::new();

    for trade in trades {
        if trade.pnl > 0.0 {
            current_wins += 1;
            if current_losses > 0 {
                loss_streaks.push(current_losses);
                current_losses = 0;
            }
        } else if trade.pnl < 0.0 {
            current_losses += 1;
            if current_wins > 0 {
                win_streaks.push(current_wins);
                current_wins = 0;
            }
        }
        max_wins = max_wins.max(current_wins);
        max_losses = max_losses.max(current_losses);
    }
    // Push final streaks
    if current_wins > 0 {
        win_streaks.push(current_wins);
    }
    if current_losses > 0 {
        loss_streaks.push(current_losses);
    }

    let avg_wins = if win_streaks.is_empty() {
        0.0
    } else {
        win_streaks.iter().sum::<usize>() as f64 / win_streaks.len() as f64
    };
    let avg_losses = if loss_streaks.is_empty() {
        0.0
    } else {
        loss_streaks.iter().sum::<usize>() as f64 / loss_streaks.len() as f64
    };

    (max_wins, max_losses, avg_wins, avg_losses)
}

/// Calculate stagnation: longest period (in bars) without making a new equity high.
fn calculate_stagnation_bars(equity_curve: &[EquityPoint]) -> usize {
    if equity_curve.len() < 2 {
        return 0;
    }
    let mut peak = equity_curve[0].equity;
    let mut current_stag = 0usize;
    let mut max_stag = 0usize;

    for point in equity_curve.iter().skip(1) {
        if point.equity > peak {
            peak = point.equity;
            current_stag = 0;
        } else {
            current_stag += 1;
            if current_stag > max_stag {
                max_stag = current_stag;
            }
        }
    }
    max_stag
}

/// Calculate Ulcer Index percentage from the equity curve.
/// UI = sqrt(mean(drawdown_pct²)) where drawdown_pct is measured from the running peak.
fn calculate_ulcer_index(equity_curve: &[EquityPoint]) -> f64 {
    if equity_curve.len() < 2 {
        return 0.0;
    }
    let mut peak = equity_curve[0].equity;
    let mut sum_sq = 0.0f64;
    let n = equity_curve.len();

    for point in equity_curve.iter() {
        if point.equity > peak {
            peak = point.equity;
        }
        let dd_pct = if peak > 0.0 {
            (peak - point.equity) / peak * 100.0
        } else {
            0.0
        };
        sum_sq += dd_pct * dd_pct;
    }
    (sum_sq / n as f64).sqrt()
}

/// Format a number of bars to a human-readable duration, given minutes per bar.
fn format_bars(bars: usize, minutes_per_bar: u32) -> String {
    let total_minutes = bars as u64 * minutes_per_bar as u64;
    if total_minutes < 60 {
        format!("{}m", total_minutes)
    } else if total_minutes < 1440 {
        format!("{}h {}m", total_minutes / 60, total_minutes % 60)
    } else {
        format!("{}d {}h", total_minutes / 1440, (total_minutes % 1440) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::config::Timeframe;
    use crate::models::strategy::TradeDirection;
    use crate::models::trade::CloseReason;

    fn make_trade(pnl: f64, duration_bars: usize) -> TradeResult {
        TradeResult {
            id: "t1".to_string(),
            direction: TradeDirection::Long,
            entry_time: "2024-01-01 00:00".to_string(),
            entry_price: 1.1000,
            exit_time: "2024-01-01 01:00".to_string(),
            exit_price: if pnl > 0.0 { 1.1050 } else { 1.0950 },
            lots: 1.0,
            pnl,
            pnl_pips: pnl / 10.0,
            commission: 0.0,
            close_reason: CloseReason::Signal,
            duration_bars,
            duration_time: format_bars(duration_bars, 1),
            mae: 5.0,
            mfe: 10.0,
        }
    }

    #[test]
    fn test_empty_metrics() {
        let m = calculate_metrics(&[], &[], 10000.0, Timeframe::M1);
        assert_eq!(m.total_trades, 0);
        assert_eq!(m.final_capital, 10000.0);
    }

    #[test]
    fn test_basic_metrics() {
        let trades = vec![
            make_trade(500.0, 10),
            make_trade(-200.0, 5),
            make_trade(300.0, 8),
        ];
        let equity_curve = vec![
            EquityPoint { timestamp: "2024-01-01".to_string(), equity: 10000.0 },
            EquityPoint { timestamp: "2024-01-02".to_string(), equity: 10500.0 },
            EquityPoint { timestamp: "2024-01-03".to_string(), equity: 10300.0 },
            EquityPoint { timestamp: "2024-01-04".to_string(), equity: 10600.0 },
        ];
        let m = calculate_metrics(&trades, &equity_curve, 10000.0, Timeframe::M1);
        assert_eq!(m.total_trades, 3);
        assert_eq!(m.winning_trades, 2);
        assert_eq!(m.losing_trades, 1);
        assert!((m.gross_profit - 800.0).abs() < 0.01);
        assert!((m.gross_loss - 200.0).abs() < 0.01);
        assert!((m.net_profit - 600.0).abs() < 0.01);
        assert!((m.profit_factor - 4.0).abs() < 0.01);
        assert!((m.win_rate_pct - 66.666).abs() < 0.01);
    }

    #[test]
    fn test_consecutive_wins_losses() {
        let trades = vec![
            make_trade(100.0, 1),
            make_trade(100.0, 1),
            make_trade(100.0, 1),
            make_trade(-50.0, 1),
            make_trade(-50.0, 1),
            make_trade(100.0, 1),
        ];
        let (max_w, max_l, avg_w, avg_l) = calculate_consecutive(&trades);
        assert_eq!(max_w, 3);
        assert_eq!(max_l, 2);
        assert!((avg_w - 2.0).abs() < 0.01); // streaks: 3, 1 → avg=2.0
        assert!((avg_l - 2.0).abs() < 0.01); // streak: 2 → avg=2.0
    }

    #[test]
    fn test_sharpe_ratio() {
        // All positive returns should give positive sharpe
        let returns = vec![0.01, 0.02, 0.01, 0.03, 0.01];
        let sharpe = calculate_sharpe(&returns, 252.0);
        assert!(sharpe > 0.0);
    }

    #[test]
    fn test_drawdown_stats() {
        let curve = vec![
            EquityPoint { timestamp: "1".to_string(), equity: 10000.0 },
            EquityPoint { timestamp: "2".to_string(), equity: 10500.0 },
            EquityPoint { timestamp: "3".to_string(), equity: 10000.0 },
            EquityPoint { timestamp: "4".to_string(), equity: 9500.0 },
            EquityPoint { timestamp: "5".to_string(), equity: 10200.0 },
        ];
        let (max_dd, _, _) = calculate_drawdown_stats(&curve);
        // Peak was 10500, trough was 9500 → DD = 1000/10500 * 100 ≈ 9.52%
        assert!((max_dd - 9.52).abs() < 0.1);
    }
}
