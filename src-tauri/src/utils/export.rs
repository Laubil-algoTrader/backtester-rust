use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;

use crate::errors::AppError;
use crate::models::result::{BacktestMetrics, BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::trade::TradeResult;

/// Write a list of trades to a CSV file.
pub fn write_trades_csv(trades: &[TradeResult], path: &Path) -> Result<(), AppError> {
    let mut wtr = csv::Writer::from_path(path)
        .map_err(|e| AppError::FileWrite(format!("Cannot create CSV: {}", e)))?;

    // Header
    wtr.write_record([
        "Direction",
        "Entry Time",
        "Entry Price",
        "Exit Time",
        "Exit Price",
        "Lots",
        "P&L",
        "P&L Pips",
        "Commission",
        "Close Reason",
        "Duration",
        "Bars",
        "MAE",
        "MFE",
    ])
    .map_err(|e| AppError::FileWrite(e.to_string()))?;

    for t in trades {
        wtr.write_record([
            &format!("{:?}", t.direction),
            &t.entry_time,
            &format!("{:.6}", t.entry_price),
            &t.exit_time,
            &format!("{:.6}", t.exit_price),
            &format!("{:.4}", t.lots),
            &format!("{:.2}", t.pnl),
            &format!("{:.1}", t.pnl_pips),
            &format!("{:.2}", t.commission),
            &format!("{:?}", t.close_reason),
            &t.duration_time,
            &t.duration_bars.to_string(),
            &format!("{:.1}", t.mae),
            &format!("{:.1}", t.mfe),
        ])
        .map_err(|e| AppError::FileWrite(e.to_string()))?;
    }

    wtr.flush().map_err(|e| AppError::FileWrite(e.to_string()))?;
    Ok(())
}

/// Write backtest metrics as a key-value CSV report.
pub fn write_metrics_csv(metrics: &BacktestMetrics, path: &Path) -> Result<(), AppError> {
    let mut wtr = csv::Writer::from_path(path)
        .map_err(|e| AppError::FileWrite(format!("Cannot create CSV: {}", e)))?;

    wtr.write_record(["Metric", "Value"])
        .map_err(|e| AppError::FileWrite(e.to_string()))?;

    let rows: Vec<(&str, String)> = vec![
        // Returns
        ("Final Capital", format!("{:.2}", metrics.final_capital)),
        ("Total Return %", format!("{:.2}", metrics.total_return_pct)),
        ("Annualized Return %", format!("{:.2}", metrics.annualized_return_pct)),
        ("Monthly Avg Return %", format!("{:.2}", metrics.monthly_return_avg_pct)),
        // Risk-adjusted
        ("Sharpe Ratio", format!("{:.2}", metrics.sharpe_ratio)),
        ("Sortino Ratio", format!("{:.2}", metrics.sortino_ratio)),
        ("Calmar Ratio", format!("{:.2}", metrics.calmar_ratio)),
        // Drawdown
        ("Max Drawdown %", format!("{:.2}", metrics.max_drawdown_pct)),
        ("Max DD Duration (bars)", metrics.max_drawdown_duration_bars.to_string()),
        ("Max DD Duration (time)", metrics.max_drawdown_duration_time.clone()),
        ("Avg Drawdown %", format!("{:.2}", metrics.avg_drawdown_pct)),
        ("Recovery Factor", format!("{:.2}", metrics.recovery_factor)),
        // Trades
        ("Total Trades", metrics.total_trades.to_string()),
        ("Winning Trades", metrics.winning_trades.to_string()),
        ("Losing Trades", metrics.losing_trades.to_string()),
        ("Breakeven Trades", metrics.breakeven_trades.to_string()),
        ("Win Rate %", format!("{:.2}", metrics.win_rate_pct)),
        // P&L
        ("Gross Profit", format!("{:.2}", metrics.gross_profit)),
        ("Gross Loss", format!("{:.2}", metrics.gross_loss)),
        ("Net Profit", format!("{:.2}", metrics.net_profit)),
        ("Profit Factor", format!("{:.2}", metrics.profit_factor)),
        ("Avg Trade", format!("{:.2}", metrics.avg_trade)),
        ("Avg Win", format!("{:.2}", metrics.avg_win)),
        ("Avg Loss", format!("{:.2}", metrics.avg_loss)),
        ("Largest Win", format!("{:.2}", metrics.largest_win)),
        ("Largest Loss", format!("{:.2}", metrics.largest_loss)),
        ("Expectancy", format!("{:.2}", metrics.expectancy)),
        // Consistency
        ("Max Consecutive Wins", metrics.max_consecutive_wins.to_string()),
        ("Max Consecutive Losses", metrics.max_consecutive_losses.to_string()),
        ("Avg Consecutive Wins", format!("{:.1}", metrics.avg_consecutive_wins)),
        ("Avg Consecutive Losses", format!("{:.1}", metrics.avg_consecutive_losses)),
        // Time
        ("Avg Trade Duration", metrics.avg_trade_duration.clone()),
        ("Avg Bars in Trade", format!("{:.1}", metrics.avg_bars_in_trade)),
        ("Avg Winner Duration", metrics.avg_winner_duration.clone()),
        ("Avg Loser Duration", metrics.avg_loser_duration.clone()),
        // Risk
        ("MAE Avg", format!("{:.1}", metrics.mae_avg)),
        ("MAE Max", format!("{:.1}", metrics.mae_max)),
        ("MFE Avg", format!("{:.1}", metrics.mfe_avg)),
        ("MFE Max", format!("{:.1}", metrics.mfe_max)),
        // Stagnation & Ulcer
        ("Stagnation (bars)", metrics.stagnation_bars.to_string()),
        ("Stagnation (time)", metrics.stagnation_time.clone()),
        ("Ulcer Index %", format!("{:.2}", metrics.ulcer_index_pct)),
        ("Return/DD Ratio", format!("{:.2}", metrics.return_dd_ratio)),
    ];

    for (name, value) in &rows {
        wtr.write_record([*name, value.as_str()])
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
    }

    wtr.flush().map_err(|e| AppError::FileWrite(e.to_string()))?;
    Ok(())
}

/// Write a full HTML backtest report with inline CSS, SVG charts, metrics, and trades table.
pub fn write_report_html(results: &BacktestResults, path: &Path) -> Result<(), AppError> {
    let mut html = String::with_capacity(256 * 1024);
    let m = &results.metrics;

    // ── HTML head ──
    write!(html, r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Backtest Report</title>
<style>
:root {{
  --bg: #09090b; --card: #18181b; --border: #27272a; --fg: #fafafa;
  --muted: #a1a1aa; --green: #22c55e; --red: #ef4444; --blue: #3b82f6;
  --accent: #6366f1;
}}
* {{ margin:0; padding:0; box-sizing:border-box; }}
body {{ background:var(--bg); color:var(--fg); font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif; padding:24px; max-width:1200px; margin:0 auto; }}
h1 {{ font-size:1.5rem; margin-bottom:8px; }}
h3 {{ font-size:0.95rem; margin-bottom:12px; color:var(--fg); }}
.timestamp {{ color:var(--muted); font-size:0.8rem; margin-bottom:24px; }}
.card {{ background:var(--card); border:1px solid var(--border); border-radius:8px; padding:16px; margin-bottom:16px; }}
.metrics-grid {{ display:grid; grid-template-columns:repeat(auto-fill,minmax(180px,1fr)); gap:8px; }}
.metric {{ background:var(--bg); border:1px solid var(--border); border-radius:6px; padding:10px; }}
.metric-label {{ font-size:0.7rem; color:var(--muted); text-transform:uppercase; letter-spacing:0.05em; }}
.metric-value {{ font-size:1rem; font-weight:600; margin-top:2px; }}
.positive {{ color:var(--green); }}
.negative {{ color:var(--red); }}
.chart-container {{ width:100%; overflow-x:auto; }}
svg {{ display:block; }}
table {{ width:100%; border-collapse:collapse; font-size:0.75rem; }}
th {{ background:var(--bg); color:var(--muted); text-align:left; padding:6px 8px; border-bottom:1px solid var(--border); font-weight:500; text-transform:uppercase; font-size:0.65rem; letter-spacing:0.05em; position:sticky; top:0; }}
td {{ padding:6px 8px; border-bottom:1px solid var(--border); }}
tr:hover td {{ background:rgba(255,255,255,0.02); }}
.long {{ color:var(--green); }}
.short {{ color:var(--red); }}
</style>
</head>
<body>
<h1>Backtest Report</h1>
<p class="timestamp">Generated: {}</p>
"#, chrono::Local::now().format("%Y-%m-%d %H:%M:%S")).ok();

    // ── Metrics Grid ──
    html.push_str(r#"<div class="card"><h3>Performance Metrics</h3><div class="metrics-grid">"#);

    let metrics_list: Vec<(&str, String, Option<bool>)> = vec![
        ("Final Capital", format!("${:.2}", m.final_capital), None),
        ("Total Return", format!("{:.2}%", m.total_return_pct), Some(m.total_return_pct >= 0.0)),
        ("Annualized Return", format!("{:.2}%", m.annualized_return_pct), Some(m.annualized_return_pct >= 0.0)),
        ("Monthly Avg Return", format!("{:.2}%", m.monthly_return_avg_pct), Some(m.monthly_return_avg_pct >= 0.0)),
        ("Sharpe Ratio", format!("{:.2}", m.sharpe_ratio), Some(m.sharpe_ratio >= 0.0)),
        ("Sortino Ratio", format!("{:.2}", m.sortino_ratio), Some(m.sortino_ratio >= 0.0)),
        ("Calmar Ratio", format!("{:.2}", m.calmar_ratio), Some(m.calmar_ratio >= 0.0)),
        ("Max Drawdown", format!("{:.2}%", m.max_drawdown_pct), Some(false)),
        ("Max DD Duration", m.max_drawdown_duration_time.clone(), None),
        ("Avg Drawdown", format!("{:.2}%", m.avg_drawdown_pct), Some(false)),
        ("Recovery Factor", format!("{:.2}", m.recovery_factor), Some(m.recovery_factor >= 1.0)),
        ("Total Trades", m.total_trades.to_string(), None),
        ("Winning Trades", m.winning_trades.to_string(), None),
        ("Losing Trades", m.losing_trades.to_string(), None),
        ("Win Rate", format!("{:.2}%", m.win_rate_pct), Some(m.win_rate_pct >= 50.0)),
        ("Gross Profit", format!("${:.2}", m.gross_profit), Some(true)),
        ("Gross Loss", format!("${:.2}", m.gross_loss), Some(false)),
        ("Net Profit", format!("${:.2}", m.net_profit), Some(m.net_profit >= 0.0)),
        ("Profit Factor", format!("{:.2}", m.profit_factor), Some(m.profit_factor >= 1.0)),
        ("Avg Trade", format!("${:.2}", m.avg_trade), Some(m.avg_trade >= 0.0)),
        ("Avg Win", format!("${:.2}", m.avg_win), Some(true)),
        ("Avg Loss", format!("${:.2}", m.avg_loss), Some(false)),
        ("Largest Win", format!("${:.2}", m.largest_win), Some(true)),
        ("Largest Loss", format!("${:.2}", m.largest_loss), Some(false)),
        ("Expectancy", format!("${:.2}", m.expectancy), Some(m.expectancy >= 0.0)),
        ("Max Consec. Wins", m.max_consecutive_wins.to_string(), None),
        ("Max Consec. Losses", m.max_consecutive_losses.to_string(), None),
        ("Avg Trade Duration", m.avg_trade_duration.clone(), None),
        ("Avg Bars in Trade", format!("{:.1}", m.avg_bars_in_trade), None),
        ("MAE Avg", format!("{:.1}", m.mae_avg), None),
        ("MAE Max", format!("{:.1}", m.mae_max), None),
        ("MFE Avg", format!("{:.1}", m.mfe_avg), None),
        ("MFE Max", format!("{:.1}", m.mfe_max), None),
        ("Stagnation", m.stagnation_time.clone(), None),
        ("Ulcer Index %", format!("{:.2}", m.ulcer_index_pct), None),
        ("Return/DD Ratio", format!("{:.2}", m.return_dd_ratio), Some(m.return_dd_ratio > 0.0)),
    ];

    for (label, value, color) in &metrics_list {
        let class = match color {
            Some(true) => " positive",
            Some(false) => " negative",
            None => "",
        };
        write!(html, r#"<div class="metric"><div class="metric-label">{}</div><div class="metric-value{}">{}</div></div>"#, label, class, value).ok();
    }
    html.push_str("</div></div>");

    // ── Equity Curve SVG ──
    html.push_str(r#"<div class="card"><h3>Equity Curve</h3><div class="chart-container">"#);
    write_equity_svg(&mut html, &results.equity_curve);
    html.push_str("</div></div>");

    // ── Drawdown SVG ──
    html.push_str(r#"<div class="card"><h3>Drawdown</h3><div class="chart-container">"#);
    write_drawdown_svg(&mut html, &results.drawdown_curve);
    html.push_str("</div></div>");

    // ── Trades Table ──
    html.push_str(r#"<div class="card"><h3>Trades</h3><div style="overflow-x:auto;max-height:600px;overflow-y:auto">"#);
    html.push_str("<table><thead><tr>");
    for h in &["#", "Dir", "Entry Time", "Entry Price", "Exit Time", "Exit Price", "Lots", "P&L", "P&L Pips", "Commission", "Reason", "Duration", "MAE", "MFE"] {
        write!(html, "<th>{}</th>", h).ok();
    }
    html.push_str("</tr></thead><tbody>");

    for (i, t) in results.trades.iter().enumerate() {
        let dir_class = match t.direction {
            crate::models::strategy::TradeDirection::Long => "long",
            crate::models::strategy::TradeDirection::Short => "short",
            _ => "",
        };
        let pnl_class = if t.pnl >= 0.0 { "positive" } else { "negative" };
        write!(html, "<tr><td>{}</td><td class=\"{}\">{:?}</td><td>{}</td><td>{:.6}</td><td>{}</td><td>{:.6}</td><td>{:.4}</td><td class=\"{}\">{:.2}</td><td>{:.1}</td><td>{:.2}</td><td>{:?}</td><td>{}</td><td>{:.1}</td><td>{:.1}</td></tr>",
            i + 1, dir_class, t.direction, t.entry_time, t.entry_price,
            t.exit_time, t.exit_price, t.lots, pnl_class, t.pnl, t.pnl_pips,
            t.commission, t.close_reason, t.duration_time, t.mae, t.mfe
        ).ok();
    }
    html.push_str("</tbody></table></div></div>");

    // ── Footer ──
    html.push_str(r#"<p style="text-align:center;color:var(--muted);font-size:0.7rem;margin-top:24px;">Generated by Backtester</p>"#);
    html.push_str("</body></html>");

    // Write to file
    let mut file = std::fs::File::create(path)
        .map_err(|e| AppError::FileWrite(format!("Cannot create HTML: {}", e)))?;
    file.write_all(html.as_bytes())
        .map_err(|e| AppError::FileWrite(e.to_string()))?;

    Ok(())
}

/// Render an SVG equity curve into the html string.
fn write_equity_svg(html: &mut String, data: &[EquityPoint]) {
    if data.is_empty() { return; }

    let w: f64 = 900.0;
    let h: f64 = 300.0;
    let pad = 60.0;
    let chart_w = w - pad - 10.0;
    let chart_h = h - 40.0;

    // Downsample
    let max_pts = 500;
    let step = (data.len() / max_pts).max(1);
    let pts: Vec<&EquityPoint> = data.iter().step_by(step).collect();
    if pts.is_empty() { return; }

    let min_eq = pts.iter().map(|p| p.equity).fold(f64::INFINITY, f64::min);
    let max_eq = pts.iter().map(|p| p.equity).fold(f64::NEG_INFINITY, f64::max);
    let range = (max_eq - min_eq).max(1.0);

    let x_step = chart_w / (pts.len() as f64 - 1.0).max(1.0);

    write!(html, r##"<svg width="100%" viewBox="0 0 {} {}" xmlns="http://www.w3.org/2000/svg" style="max-width:{}px">"##, w, h, w as i64).ok();

    // Grid lines
    for i in 0..5 {
        let y = 10.0 + chart_h * (i as f64 / 4.0);
        let val = max_eq - range * (i as f64 / 4.0);
        write!(html, r##"<line x1="{}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="#27272a" stroke-dasharray="3,3"/>"##, pad, y, w - 10.0, y).ok();
        write!(html, r##"<text x="{}" y="{:.1}" fill="#a1a1aa" font-size="10" text-anchor="end">${}</text>"##, pad - 4.0, y + 3.0, format_number(val)).ok();
    }

    // Line path
    let mut path = String::with_capacity(pts.len() * 20);
    for (i, pt) in pts.iter().enumerate() {
        let x = pad + x_step * i as f64;
        let y = 10.0 + chart_h * (1.0 - (pt.equity - min_eq) / range);
        if i == 0 { write!(path, "M{:.1},{:.1}", x, y).ok(); }
        else { write!(path, " L{:.1},{:.1}", x, y).ok(); }
    }
    write!(html, r##"<path d="{}" fill="none" stroke="#3b82f6" stroke-width="1.5"/>"##, path).ok();

    // Fill area
    let x_end = pad + x_step * (pts.len() - 1) as f64;
    write!(html, r##"<path d="{} L{:.1},{:.1} L{:.1},{:.1} Z" fill="#3b82f6" fill-opacity="0.1"/>"##,
        path, x_end, 10.0 + chart_h, pad, 10.0 + chart_h).ok();

    html.push_str("</svg>");
}

/// Render an SVG drawdown chart into the html string.
fn write_drawdown_svg(html: &mut String, data: &[DrawdownPoint]) {
    if data.is_empty() { return; }

    let w: f64 = 900.0;
    let h: f64 = 160.0;
    let pad = 60.0;
    let chart_w = w - pad - 10.0;
    let chart_h = h - 30.0;

    let max_pts = 500;
    let step = (data.len() / max_pts).max(1);
    let pts: Vec<&DrawdownPoint> = data.iter().step_by(step).collect();
    if pts.is_empty() { return; }

    let min_dd = pts.iter().map(|p| p.drawdown_pct).fold(f64::INFINITY, f64::min);
    let max_dd = 0.0_f64;
    let range = (max_dd - min_dd).max(0.01);

    let x_step = chart_w / (pts.len() as f64 - 1.0).max(1.0);

    write!(html, r##"<svg width="100%" viewBox="0 0 {} {}" xmlns="http://www.w3.org/2000/svg" style="max-width:{}px">"##, w, h, w as i64).ok();

    // Zero line
    write!(html, r##"<line x1="{}" y1="10" x2="{}" y2="10" stroke="#27272a"/>"##, pad, w - 10.0).ok();

    // Grid
    for i in 1..4 {
        let y = 10.0 + chart_h * (i as f64 / 3.0);
        let val = min_dd * (i as f64 / 3.0);
        write!(html, r##"<line x1="{}" y1="{:.1}" x2="{}" y2="{:.1}" stroke="#27272a" stroke-dasharray="3,3"/>"##, pad, y, w - 10.0, y).ok();
        write!(html, r##"<text x="{}" y="{:.1}" fill="#a1a1aa" font-size="10" text-anchor="end">{:.1}%</text>"##, pad - 4.0, y + 3.0, val).ok();
    }

    // Area path
    let mut path = String::with_capacity(pts.len() * 20);
    for (i, pt) in pts.iter().enumerate() {
        let x = pad + x_step * i as f64;
        let y = 10.0 + chart_h * ((max_dd - pt.drawdown_pct) / range);
        if i == 0 { write!(path, "M{:.1},{:.1}", x, y).ok(); }
        else { write!(path, " L{:.1},{:.1}", x, y).ok(); }
    }

    let x_end = pad + x_step * (pts.len() - 1) as f64;
    write!(html, r##"<path d="{} L{:.1},10 L{:.1},10 Z" fill="#ef4444" fill-opacity="0.3"/>"##, path, x_end, pad).ok();
    write!(html, r##"<path d="{}" fill="none" stroke="#ef4444" stroke-width="1.5"/>"##, path).ok();

    html.push_str("</svg>");
}

/// Format a number with thousands separator for chart labels.
fn format_number(v: f64) -> String {
    let abs = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if abs >= 1_000_000.0 {
        format!("{}{:.1}M", sign, abs / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{}{:.0}K", sign, abs / 1_000.0)
    } else {
        format!("{}{:.0}", sign, abs)
    }
}
