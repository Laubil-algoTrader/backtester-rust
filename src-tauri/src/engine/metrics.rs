use chrono::NaiveDate;

use crate::models::config::Timeframe;
use crate::models::result::{BacktestMetrics, EquityPoint, MonthlyReturn};
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

    // ── Trade classification + P&L (single pass) ──
    // Thresholds ensure mutual exclusivity: |pnl| < 1e-6 → breakeven, avoiding
    // double-counting trades where pnl is near-zero but technically > 0 or < 0.
    let mut winning_trades = 0usize;
    let mut losing_trades = 0usize;
    let mut breakeven_trades = 0usize;
    let mut gross_profit = 0.0f64;
    let mut gross_loss = 0.0f64;
    let mut largest_win = 0.0f64;
    let mut largest_loss = 0.0f64;
    let mut total_commission = 0.0f64;
    let mut total_swap = 0.0f64;
    let mut sum_pnl = 0.0f64;
    let mut winner_bars_sum = 0usize;
    let mut loser_bars_sum = 0usize;
    let mut total_bars_sum = 0usize;
    let mut mae_sum = 0.0f64;
    let mut mae_max_acc = 0.0f64;
    let mut mfe_sum = 0.0f64;
    let mut mfe_max_acc = 0.0f64;

    for t in trades.iter() {
        sum_pnl += t.pnl;
        total_commission += t.commission;
        total_swap += t.swap;
        total_bars_sum += t.duration_bars;
        mae_sum += t.mae;
        mfe_sum += t.mfe;
        if t.mae > mae_max_acc { mae_max_acc = t.mae; }
        if t.mfe > mfe_max_acc { mfe_max_acc = t.mfe; }

        if t.pnl >= 1e-6 {
            winning_trades += 1;
            gross_profit += t.pnl;
            if t.pnl > largest_win { largest_win = t.pnl; }
            winner_bars_sum += t.duration_bars;
        } else if t.pnl <= -1e-6 {
            losing_trades += 1;
            gross_loss += t.pnl.abs();
            if t.pnl < largest_loss { largest_loss = t.pnl; }
            loser_bars_sum += t.duration_bars;
        } else {
            breakeven_trades += 1;
        }
    }

    let win_rate_pct = winning_trades as f64 / total_trades as f64 * 100.0;
    let net_profit = sum_pnl - total_commission + total_swap;
    let avg_trade = sum_pnl / total_trades as f64;

    // Cap at 999 when there are no losing trades — serde_json cannot serialize f64::INFINITY.
    let profit_factor = if gross_loss > 0.0 {
        gross_profit / gross_loss
    } else if gross_profit > 0.0 {
        999.0
    } else {
        0.0
    };

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
    let expectancy = (win_rate_pct / 100.0) * avg_win + (1.0 - win_rate_pct / 100.0) * avg_loss;

    // ── Returns ──
    let final_capital = initial_capital + net_profit;
    let total_return_pct = net_profit / initial_capital * 100.0;

    // Annualized return: estimate trading days from equity curve using actual timeframe
    let trading_bars = equity_curve.len().max(1);
    let bpd = bars_per_day(timeframe);
    let annualized_return_pct = annualize_return(total_return_pct, trading_bars, bpd, equity_curve);
    let bars_per_month = bpd * 21.0; // ~21 trading days per month
    let monthly_return_avg_pct = if trading_bars > 0 {
        total_return_pct / (trading_bars as f64 / bars_per_month).max(1.0)
    } else {
        0.0
    };

    // ── Drawdown ──
    let (max_drawdown_pct, max_dd_duration_bars, avg_drawdown_pct) =
        calculate_drawdown_stats(equity_curve);
    // Recovery Factor = Net Profit / Max Absolute Drawdown
    // Computed directly from the equity curve to avoid dependence on initial_capital.
    let max_dd_absolute = if !equity_curve.is_empty() {
        let mut peak = equity_curve[0].equity;
        let mut max_abs = 0.0f64;
        for point in equity_curve {
            if point.equity > peak {
                peak = point.equity;
            }
            let abs_dd = peak - point.equity;
            if abs_dd > max_abs {
                max_abs = abs_dd;
            }
        }
        max_abs
    } else {
        0.0
    };
    let recovery_factor = if max_dd_absolute > 0.0 {
        net_profit / max_dd_absolute
    } else {
        0.0
    };

    // ── Risk-adjusted ──
    // Prefer daily equity returns for Sharpe/Sortino/Omega — this gives a methodology-consistent
    // result regardless of trade frequency. Fall back to per-trade returns only when the
    // equity curve spans fewer than 2 calendar days (very short tests).
    let daily_returns_cache = equity_to_daily_returns(equity_curve);
    let (sharpe_ratio, sortino_ratio) =
        if let Some((ref daily_returns, n_days)) = daily_returns_cache {
            // Trading days per year = observed days / calendar years.
            // This adapts automatically: ~252 for stocks, ~260 for forex, ~365 for crypto.
            let cal_years = calendar_years_from_equity(equity_curve).unwrap_or(1.0);
            let trading_days_per_year = (n_days as f64 / cal_years).max(1.0);
            (
                calculate_sharpe(daily_returns, trading_days_per_year),
                calculate_sortino(daily_returns, trading_days_per_year),
            )
        } else {
            // Fallback: per-trade returns with frequency-adjusted annualization factor.
            let bars_per_year = 252.0 * bpd;
            let annualization_factor = if trading_bars > 0 && total_trades >= 2 {
                (total_trades as f64 * bars_per_year / trading_bars as f64).max(1.0)
            } else {
                bars_per_year
            };
            let trade_returns: Vec<f64> =
                trades.iter().map(|t| t.pnl / initial_capital).collect();
            (
                calculate_sharpe(&trade_returns, annualization_factor),
                calculate_sortino(&trade_returns, annualization_factor),
            )
        };
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
    let avg_bars_in_trade = total_bars_sum as f64 / total_trades as f64;
    let avg_trade_duration = format_bars(avg_bars_in_trade as usize, mpb);

    let avg_winner_bars = if winning_trades > 0 {
        winner_bars_sum as f64 / winning_trades as f64
    } else {
        0.0
    };
    let avg_loser_bars = if losing_trades > 0 {
        loser_bars_sum as f64 / losing_trades as f64
    } else {
        0.0
    };

    // ── Risk (MAE/MFE) — values already accumulated in the single pass above ──
    let mae_avg = mae_sum / total_trades as f64;
    let mae_max = mae_max_acc;
    let mfe_avg = mfe_sum / total_trades as f64;
    let mfe_max = mfe_max_acc;

    // ── Stagnation (longest period without new equity high) ──
    let stagnation_bars = calculate_stagnation_bars(equity_curve);
    let stagnation_time = format_bars(stagnation_bars, mpb);

    // ── Ulcer Index % ──
    let ulcer_index_pct = calculate_ulcer_index(equity_curve);

    // ── Additional metrics ──
    let k_ratio = calculate_k_ratio(equity_curve);
    let omega_ratio = if let Some((ref daily_returns, _)) = daily_returns_cache {
        calculate_omega_ratio(daily_returns, 0.0)
    } else {
        0.0
    };
    let monthly_returns = compute_monthly_returns(equity_curve);

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
            999.0 // cap — serde_json cannot serialize f64::INFINITY
        } else {
            0.0
        },
        k_ratio,
        omega_ratio,
        monthly_returns,
        total_swap_charged: total_swap,
        total_commission_charged: total_commission,
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
        k_ratio: 0.0,
        omega_ratio: 0.0,
        monthly_returns: vec![],
        total_swap_charged: 0.0,
        total_commission_charged: 0.0,
    }
}

/// Extract the number of calendar years spanned by an equity curve using actual timestamps.
///
/// Parses the first 10 characters of each endpoint timestamp as `YYYY-MM-DD`.
/// Returns `None` if parsing fails or the span is zero.
fn calendar_years_from_equity(equity_curve: &[EquityPoint]) -> Option<f64> {
    if equity_curve.len() < 2 {
        return None;
    }
    let first_ts = &equity_curve.first()?.timestamp;
    let last_ts = &equity_curve.last()?.timestamp;
    // Accept both "YYYY-MM-DD" and "YYYY-MM-DD HH:MM:SS" by slicing the date prefix.
    let parse_date = |s: &str| -> Option<NaiveDate> {
        let date_part = s.get(..10)?;
        NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
    };
    let first_date = parse_date(first_ts)?;
    let last_date = parse_date(last_ts)?;
    let days = (last_date - first_date).num_days();
    if days <= 0 {
        return None;
    }
    Some(days as f64 / 365.25)
}

/// Annualize a return percentage.
///
/// Uses actual calendar days from the equity curve when available (preferred), falling back
/// to bar-count estimation based on the timeframe's bars-per-day.
fn annualize_return(total_return_pct: f64, bars: usize, bpd: f64, equity_curve: &[EquityPoint]) -> f64 {
    let years = if let Some(cal_years) = calendar_years_from_equity(equity_curve) {
        cal_years
    } else {
        if bars == 0 {
            return 0.0;
        }
        let bars_per_year = 252.0 * bpd;
        bars as f64 / bars_per_year
    };
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

/// Convert an equity curve to daily percentage returns and observed trading-day count.
///
/// Groups equity curve points by their calendar date (first 10 chars of timestamp,
/// assumed to be `YYYY-MM-DD`). Takes the last equity value of each day and computes
/// day-over-day returns. Returns `None` when fewer than 2 trading days are present.
///
/// The second element of the tuple is the count of unique trading days, which callers
/// use to derive `trading_days_per_year` for the annualization factor.
fn equity_to_daily_returns(equity_curve: &[EquityPoint]) -> Option<(Vec<f64>, usize)> {
    use std::collections::BTreeMap;

    if equity_curve.len() < 2 {
        return None;
    }

    // Last equity value per calendar day (BTreeMap keeps keys in lexicographic/date order)
    let mut by_day: BTreeMap<String, f64> = BTreeMap::new();
    for pt in equity_curve {
        let date: String = pt.timestamp.chars().take(10).collect();
        by_day.insert(date, pt.equity);
    }

    let daily: Vec<f64> = by_day.values().copied().collect();
    let n_days = daily.len();
    if n_days < 2 {
        return None;
    }

    let returns: Vec<f64> = daily
        .windows(2)
        .filter_map(|w| {
            if w[0] > 0.0 {
                Some((w[1] - w[0]) / w[0])
            } else {
                None
            }
        })
        .collect();

    if returns.len() < 2 {
        return None;
    }

    Some((returns, n_days))
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
    // Downside deviation uses ALL n observations in the denominator (not just the negative ones).
    // This is the standard Sortino formula — negative returns contribute their squared value,
    // positive returns contribute 0. Dividing by neg_count instead of n would overstate the
    // downside risk, making the ratio appear worse than it really is.
    let downside_sum: f64 = negative_returns.iter().map(|r| r.powi(2)).sum();
    let downside_dev = (downside_sum / n as f64).sqrt();
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
        if trade.pnl > 1e-9 {
            current_wins += 1;
            if current_losses > 0 {
                loss_streaks.push(current_losses);
                current_losses = 0;
            }
        } else if trade.pnl < -1e-9 {
            current_losses += 1;
            if current_wins > 0 {
                win_streaks.push(current_wins);
                current_wins = 0;
            }
        } else {
            // Breakeven trade — flush both running streaks
            if current_wins > 0 {
                win_streaks.push(current_wins);
                current_wins = 0;
            }
            if current_losses > 0 {
                loss_streaks.push(current_losses);
                current_losses = 0;
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

/// K-Ratio: measures the consistency of the equity curve growth.
///
/// Fits a linear regression on `log(equity[i] / equity[0])` vs bar index i.
/// Returns `slope / std_error_of_slope * sqrt(n)`, normalized to be comparable across
/// strategies. Higher is better; values > 1.0 indicate consistent growth.
fn calculate_k_ratio(equity_curve: &[EquityPoint]) -> f64 {
    let n = equity_curve.len();
    if n < 4 {
        return 0.0;
    }
    let base = equity_curve[0].equity;
    if base <= 0.0 {
        return 0.0;
    }

    // y[i] = log(equity[i] / base), x[i] = i
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_xx = 0.0f64;
    let mut sum_xy = 0.0f64;
    let mut valid = 0usize;

    for (i, pt) in equity_curve.iter().enumerate() {
        if pt.equity <= 0.0 {
            continue;
        }
        let x = i as f64;
        let y = (pt.equity / base).ln();
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
        valid += 1;
    }

    if valid < 4 {
        return 0.0;
    }
    let nf = valid as f64;
    let denom = nf * sum_xx - sum_x * sum_x;
    if denom.abs() < f64::EPSILON {
        return 0.0;
    }

    let slope = (nf * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / nf;

    // Residual sum of squares
    let mut ss_res = 0.0f64;
    for (i, pt) in equity_curve.iter().enumerate() {
        if pt.equity <= 0.0 {
            continue;
        }
        let y = (pt.equity / base).ln();
        let y_hat = intercept + slope * i as f64;
        ss_res += (y - y_hat).powi(2);
    }

    let var_slope = if valid > 2 {
        let mean_x = sum_x / nf;
        let ss_xx = sum_xx - nf * mean_x * mean_x;
        if ss_xx > 0.0 {
            (ss_res / (nf - 2.0)) / ss_xx
        } else {
            return 0.0;
        }
    } else {
        return 0.0;
    };

    let std_err = var_slope.sqrt();
    if std_err < f64::EPSILON {
        return 0.0;
    }
    (slope / std_err) * (nf.sqrt())
}

/// Omega Ratio: probability-weighted ratio of gains to losses above/below threshold.
///
/// `omega = sum(max(r - threshold, 0)) / sum(max(threshold - r, 0))` over daily returns.
/// Values > 1.0 mean more probability-weighted gain than loss.
fn calculate_omega_ratio(returns: &[f64], threshold: f64) -> f64 {
    let gains: f64 = returns.iter().map(|r| (r - threshold).max(0.0)).sum();
    let losses: f64 = returns.iter().map(|r| (threshold - r).max(0.0)).sum();
    if losses < f64::EPSILON {
        if gains > 0.0 { 999.0 } else { 0.0 } // cap — serde_json cannot serialize f64::INFINITY
    } else {
        gains / losses
    }
}

/// Compute monthly returns from the equity curve.
///
/// Groups equity-curve points by YYYY-MM calendar month and returns the
/// percentage return from the first to the last equity value in each month.
fn compute_monthly_returns(equity_curve: &[EquityPoint]) -> Vec<MonthlyReturn> {
    use std::collections::BTreeMap;

    if equity_curve.len() < 2 {
        return vec![];
    }

    // (year, month) → (first_equity, last_equity)
    let mut by_month: BTreeMap<(i32, u32), (f64, f64)> = BTreeMap::new();

    for pt in equity_curve {
        let ts = &pt.timestamp;
        if ts.len() < 7 {
            continue;
        }
        // Parse YYYY-MM from timestamp prefix
        let year: i32 = ts[0..4].parse().unwrap_or(0);
        let month: u32 = ts[5..7].parse().unwrap_or(0);
        if year == 0 || month == 0 {
            continue;
        }
        let entry = by_month.entry((year, month)).or_insert((pt.equity, pt.equity));
        entry.1 = pt.equity; // keep updating last
    }

    by_month
        .into_iter()
        .filter_map(|((year, month), (first, last))| {
            if first > 0.0 {
                Some(MonthlyReturn {
                    year,
                    month,
                    return_pct: (last - first) / first * 100.0,
                })
            } else {
                None
            }
        })
        .collect()
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
