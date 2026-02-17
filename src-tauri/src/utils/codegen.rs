use std::collections::HashSet;
use std::fmt::Write as FmtWrite;

use serde::Serialize;

use crate::errors::AppError;
use crate::models::strategy::*;

// ══════════════════════════════════════════════════════════════
// Public API — types
// ══════════════════════════════════════════════════════════════

/// A single generated code file.
#[derive(Debug, Clone, Serialize)]
pub struct CodeFile {
    pub filename: String,
    pub code: String,
    /// Whether this is the main file (EA / strategy script).
    pub is_main: bool,
}

/// Result of code generation — may contain multiple files.
#[derive(Debug, Clone, Serialize)]
pub struct CodeGenerationResult {
    pub files: Vec<CodeFile>,
}

// ══════════════════════════════════════════════════════════════
// Public API
// ══════════════════════════════════════════════════════════════

/// Generate MQL5 EA + custom indicator files from a strategy.
pub fn generate_mql5(strategy: &Strategy) -> Result<CodeGenerationResult, AppError> {
    let mut out = String::with_capacity(8192);
    let indicators = collect_unique_indicators(strategy);

    mql5_header(&mut out, strategy);
    mql5_inputs(&mut out, strategy, &indicators);
    mql5_globals(&mut out, &indicators);
    mql5_on_init(&mut out, &indicators);
    mql5_on_deinit(&mut out, &indicators);
    mql5_on_tick(&mut out, strategy);
    mql5_check_rules_fn(&mut out, &strategy.long_entry_rules, "CheckLongEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_entry_rules, "CheckShortEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.long_exit_rules, "CheckLongExit", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_exit_rules, "CheckShortExit", &indicators);
    mql5_open_position(&mut out, "Long", "ORDER_TYPE_BUY", "SYMBOL_ASK");
    mql5_open_position(&mut out, "Short", "ORDER_TYPE_SELL", "SYMBOL_BID");
    mql5_close_position(&mut out);
    mql5_lot_size(&mut out, strategy);
    mql5_sl_tp_helpers(&mut out, strategy);
    mql5_trailing_stop(&mut out, strategy);

    let ea_name = strategy.name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");

    // Collect unique indicator types used
    let mut indicator_types_used = HashSet::new();
    for ind in &indicators {
        indicator_types_used.insert(ind.config.indicator_type);
    }

    // Generate custom indicator files
    let mut files = Vec::new();
    for ind_type in &indicator_types_used {
        if let Some((filename, code)) = generate_custom_indicator(*ind_type) {
            files.push(CodeFile { filename, code, is_main: false });
        }
    }

    files.push(CodeFile {
        filename: format!("{}.mq5", ea_name),
        code: out,
        is_main: true,
    });

    Ok(CodeGenerationResult { files })
}

/// Generate a PineScript v6 strategy from a strategy.
pub fn generate_pinescript(strategy: &Strategy) -> Result<CodeGenerationResult, AppError> {
    let mut out = String::with_capacity(4096);
    let indicators = collect_unique_indicators(strategy);

    pine_header(&mut out, strategy);
    pine_inputs(&mut out, strategy, &indicators);
    pine_indicators(&mut out, &indicators);
    pine_trading_hours(&mut out, strategy);
    pine_conditions(&mut out, strategy);
    pine_execution(&mut out, strategy);
    pine_sl_tp(&mut out, strategy);
    pine_plots(&mut out, &indicators, strategy);

    let name = strategy.name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
    Ok(CodeGenerationResult {
        files: vec![CodeFile {
            filename: format!("{}.pine", name),
            code: out,
            is_main: true,
        }],
    })
}

// ══════════════════════════════════════════════════════════════
// Shared helpers
// ══════════════════════════════════════════════════════════════

/// Unique indicator instance (deduped by type + params, ignoring output_field).
struct UniqueIndicator {
    config: IndicatorConfig,
    var_name: String,
    /// MQL5 handle variable name.
    handle_name: String,
}

fn collect_unique_indicators(strategy: &Strategy) -> Vec<UniqueIndicator> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    let all_rules = strategy.long_entry_rules.iter()
        .chain(&strategy.short_entry_rules)
        .chain(&strategy.long_exit_rules)
        .chain(&strategy.short_exit_rules);

    for rule in all_rules {
        for operand in [&rule.left_operand, &rule.right_operand] {
            if operand.operand_type == OperandType::Indicator {
                if let Some(ind) = &operand.indicator {
                    let key = ind.cache_key();
                    if seen.insert(key) {
                        let var = indicator_var_name(ind);
                        let handle = format!("handle_{}", var);
                        result.push(UniqueIndicator {
                            config: ind.clone(),
                            var_name: var,
                            handle_name: handle,
                        });
                    }
                }
            }
        }
    }

    // Also collect ATR if used by SL/TP/TS
    if let Some(sl) = &strategy.stop_loss {
        if sl.sl_type == StopLossType::ATR {
            let period = sl.atr_period.unwrap_or(14);
            add_atr_if_missing(&mut seen, &mut result, period);
        }
    }
    if let Some(tp) = &strategy.take_profit {
        if tp.tp_type == TakeProfitType::ATR {
            let period = tp.atr_period.unwrap_or(14);
            add_atr_if_missing(&mut seen, &mut result, period);
        }
    }
    if let Some(ts) = &strategy.trailing_stop {
        if ts.ts_type == TrailingStopType::ATR {
            let period = ts.atr_period.unwrap_or(14);
            add_atr_if_missing(&mut seen, &mut result, period);
        }
    }

    result
}

fn add_atr_if_missing(seen: &mut HashSet<String>, result: &mut Vec<UniqueIndicator>, period: usize) {
    let config = IndicatorConfig {
        indicator_type: IndicatorType::ATR,
        params: IndicatorParams { period: Some(period), ..Default::default() },
        output_field: None,
    };
    let key = config.cache_key();
    if seen.insert(key) {
        let var = indicator_var_name(&config);
        let handle = format!("handle_{}", var);
        result.push(UniqueIndicator { config, var_name: var, handle_name: handle });
    }
}

/// Format a float for use in a variable name: replace '.' with 'p' (e.g., 2.0 → "2p0").
fn float_to_var(v: f64) -> String {
    if v == v.floor() && v.abs() < 1_000_000.0 {
        format!("{}", v as i64)
    } else {
        format!("{:.2}", v).replace('.', "p").trim_end_matches('0').trim_end_matches('p').to_string()
    }
}

fn indicator_var_name(ind: &IndicatorConfig) -> String {
    let name = match ind.indicator_type {
        IndicatorType::SMA => "sma",
        IndicatorType::EMA => "ema",
        IndicatorType::RSI => "rsi",
        IndicatorType::MACD => "macd",
        IndicatorType::BollingerBands => "bb",
        IndicatorType::ATR => "atr",
        IndicatorType::Stochastic => "stoch",
        IndicatorType::ADX => "adx",
        IndicatorType::CCI => "cci",
        IndicatorType::ROC => "roc",
        IndicatorType::WilliamsR => "wpr",
        IndicatorType::ParabolicSAR => "sar",
        IndicatorType::VWAP => "vwap",
    };

    let mut s = String::from(name);
    if let Some(p) = ind.params.period { write!(s, "_{}", p).ok(); }
    if let Some(p) = ind.params.fast_period { write!(s, "_f{}", p).ok(); }
    if let Some(p) = ind.params.slow_period { write!(s, "_s{}", p).ok(); }
    if let Some(p) = ind.params.signal_period { write!(s, "_sig{}", p).ok(); }
    if let Some(p) = ind.params.k_period { write!(s, "_k{}", p).ok(); }
    if let Some(p) = ind.params.d_period { write!(s, "_d{}", p).ok(); }
    if let Some(v) = ind.params.std_dev { write!(s, "_sd{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.acceleration_factor { write!(s, "_af{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.maximum_factor { write!(s, "_mf{}", float_to_var(v)).ok(); }
    s
}

/// Get the MQL5 buffer index for a given output_field.
fn mql5_buffer_index(ind: &IndicatorConfig) -> usize {
    let field = ind.output_field.as_deref().unwrap_or("");
    match ind.indicator_type {
        IndicatorType::MACD => match field {
            "signal" => 1,
            "histogram" => 2,
            _ => 0, // "macd" or default
        },
        IndicatorType::BollingerBands => match field {
            "upper" => 1,
            "lower" => 2,
            _ => 0, // "middle" or default
        },
        IndicatorType::Stochastic => match field {
            "D" | "d" => 1,
            _ => 0, // "K" or default
        },
        IndicatorType::ADX => match field {
            "+DI" | "plus_di" => 1,
            "-DI" | "minus_di" => 2,
            _ => 0, // "adx" or default
        },
        _ => 0,
    }
}

/// Get the PineScript variable suffix for a multi-output indicator.
fn pine_output_suffix(ind: &IndicatorConfig) -> &str {
    let field = ind.output_field.as_deref().unwrap_or("");
    match ind.indicator_type {
        IndicatorType::MACD => match field {
            "signal" => "_signal",
            "histogram" => "_hist",
            _ => "_line",
        },
        IndicatorType::BollingerBands => match field {
            "upper" => "_upper",
            "lower" => "_lower",
            _ => "_basis",
        },
        IndicatorType::Stochastic => match field {
            "D" | "d" => "_d",
            _ => "_k",
        },
        _ => "",
    }
}

fn is_multi_output(ind_type: IndicatorType) -> bool {
    matches!(ind_type, IndicatorType::MACD | IndicatorType::BollingerBands | IndicatorType::Stochastic)
}

// ══════════════════════════════════════════════════════════════
// MQL5 Generation
// ══════════════════════════════════════════════════════════════

fn mql5_header(out: &mut String, strategy: &Strategy) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "//|                         {}.mq5", strategy.name).ok();
    writeln!(out, "//|                    Generated by Backtester Rust").ok();
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "#property copyright \"Generated by Backtester Rust\"").ok();
    writeln!(out, "#property version   \"1.00\"").ok();
    writeln!(out, "#property strict").ok();
    writeln!(out, "#include <Trade/Trade.mqh>").ok();
    writeln!(out).ok();
}

fn mql5_inputs(out: &mut String, strategy: &Strategy, indicators: &[UniqueIndicator]) {
    writeln!(out, "// ═══════════════ INPUT PARAMETERS ═══════════════").ok();
    writeln!(out, "input int    InpMagicNumber = 123456;   // Magic Number").ok();

    // Position sizing
    match strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots =>
            writeln!(out, "input double InpLotSize = {:.2};       // Lot Size", strategy.position_sizing.value).ok(),
        PositionSizingType::FixedAmount =>
            writeln!(out, "input double InpFixedAmount = {:.0};   // Fixed Amount ($)", strategy.position_sizing.value).ok(),
        PositionSizingType::PercentEquity =>
            writeln!(out, "input double InpRiskPct = {:.1};       // Risk % of Equity", strategy.position_sizing.value).ok(),
        PositionSizingType::RiskBased =>
            writeln!(out, "input double InpRiskPct = {:.1};       // Risk % per Trade", strategy.position_sizing.value).ok(),
    };

    // SL/TP
    if let Some(sl) = &strategy.stop_loss {
        match sl.sl_type {
            StopLossType::Pips => writeln!(out, "input double InpSLPips = {:.1};       // Stop Loss (pips)", sl.value).ok(),
            StopLossType::Percentage => writeln!(out, "input double InpSLPct = {:.2};       // Stop Loss (%)", sl.value).ok(),
            StopLossType::ATR => writeln!(out, "input double InpSLAtrMult = {:.1};    // Stop Loss (ATR multiplier)", sl.value).ok(),
        };
    }
    if let Some(tp) = &strategy.take_profit {
        match tp.tp_type {
            TakeProfitType::Pips => writeln!(out, "input double InpTPPips = {:.1};       // Take Profit (pips)", tp.value).ok(),
            TakeProfitType::RiskReward => writeln!(out, "input double InpTPRR = {:.1};        // Take Profit (Risk:Reward)", tp.value).ok(),
            TakeProfitType::ATR => writeln!(out, "input double InpTPAtrMult = {:.1};    // Take Profit (ATR multiplier)", tp.value).ok(),
        };
    }
    if let Some(ts) = &strategy.trailing_stop {
        match ts.ts_type {
            TrailingStopType::ATR => writeln!(out, "input double InpTSAtrMult = {:.1};    // Trailing Stop (ATR mult)", ts.value).ok(),
            TrailingStopType::RiskReward => writeln!(out, "input double InpTSRR = {:.1};        // Trailing Stop (R:R)", ts.value).ok(),
        };
    }

    // Indicator params as inputs
    for ind in indicators {
        let p = &ind.config.params;
        match ind.config.indicator_type {
            IndicatorType::SMA | IndicatorType::EMA | IndicatorType::RSI |
            IndicatorType::ATR | IndicatorType::ADX | IndicatorType::CCI |
            IndicatorType::ROC | IndicatorType::WilliamsR => {
                if let Some(period) = p.period {
                    writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, period).ok();
                }
            }
            IndicatorType::MACD => {
                writeln!(out, "input int    Inp_{}_fast = {};", ind.var_name, p.fast_period.unwrap_or(12)).ok();
                writeln!(out, "input int    Inp_{}_slow = {};", ind.var_name, p.slow_period.unwrap_or(26)).ok();
                writeln!(out, "input int    Inp_{}_signal = {};", ind.var_name, p.signal_period.unwrap_or(9)).ok();
            }
            IndicatorType::BollingerBands => {
                writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, p.period.unwrap_or(20)).ok();
                writeln!(out, "input double Inp_{}_stddev = {:.1};", ind.var_name, p.std_dev.unwrap_or(2.0)).ok();
            }
            IndicatorType::Stochastic => {
                writeln!(out, "input int    Inp_{}_k = {};", ind.var_name, p.k_period.unwrap_or(14)).ok();
                writeln!(out, "input int    Inp_{}_d = {};", ind.var_name, p.d_period.unwrap_or(3)).ok();
            }
            IndicatorType::ParabolicSAR => {
                writeln!(out, "input double Inp_{}_af = {:.2};", ind.var_name, p.acceleration_factor.unwrap_or(0.02)).ok();
                writeln!(out, "input double Inp_{}_max = {:.2};", ind.var_name, p.maximum_factor.unwrap_or(0.20)).ok();
            }
            IndicatorType::VWAP => {
                writeln!(out, "// NOTE: VWAP requires custom implementation in MQL5").ok();
            }
        }
    }

    // Trading hours
    if let Some(th) = &strategy.trading_hours {
        writeln!(out, "input int    InpStartHour = {};", th.start_hour).ok();
        writeln!(out, "input int    InpStartMinute = {};", th.start_minute).ok();
        writeln!(out, "input int    InpEndHour = {};", th.end_hour).ok();
        writeln!(out, "input int    InpEndMinute = {};", th.end_minute).ok();
    }
    if let Some(max) = strategy.max_daily_trades {
        writeln!(out, "input int    InpMaxDailyTrades = {};", max).ok();
    }

    writeln!(out).ok();
}

fn mql5_globals(out: &mut String, indicators: &[UniqueIndicator]) {
    writeln!(out, "// ═══════════════ GLOBAL VARIABLES ═══════════════").ok();
    writeln!(out, "CTrade trade;").ok();
    for ind in indicators {
        writeln!(out, "int {};", ind.handle_name).ok();
    }
    writeln!(out, "int dailyTradeCount = 0;").ok();
    writeln!(out, "datetime lastTradeDay = 0;").ok();
    writeln!(out).ok();
}

fn mql5_on_init(out: &mut String, indicators: &[UniqueIndicator]) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "int OnInit()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   trade.SetExpertMagicNumber(InpMagicNumber);").ok();
    writeln!(out).ok();

    for ind in indicators {
        let call = match ind.config.indicator_type {
            IndicatorType::SMA => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_SMA\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::EMA => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_EMA\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::RSI => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_RSI\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::MACD => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_MACD\", Inp_{0}_fast, Inp_{0}_slow, Inp_{0}_signal)",
                ind.var_name
            ),
            IndicatorType::BollingerBands => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BollingerBands\", Inp_{0}_period, Inp_{0}_stddev)",
                ind.var_name
            ),
            IndicatorType::ATR => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ATR\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::Stochastic => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Stochastic\", Inp_{0}_k, Inp_{0}_d)",
                ind.var_name
            ),
            IndicatorType::ADX => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ADX\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::CCI => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_CCI\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::WilliamsR => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_WilliamsR\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::ParabolicSAR => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ParabolicSAR\", Inp_{0}_af, Inp_{0}_max)",
                ind.var_name
            ),
            IndicatorType::ROC => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ROC\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::VWAP => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_VWAP\")"
            ),
        };

        writeln!(out, "   {} = {};", ind.handle_name, call).ok();
        writeln!(out, "   if({} == INVALID_HANDLE) {{ Print(\"Failed to create {} handle\"); return INIT_FAILED; }}", ind.handle_name, ind.var_name).ok();
        writeln!(out).ok();
    }

    writeln!(out, "   Print(\"EA initialized: {}\");", "OK").ok();
    writeln!(out, "   return INIT_SUCCEEDED;").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_on_deinit(out: &mut String, indicators: &[UniqueIndicator]) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void OnDeinit(const int reason)").ok();
    writeln!(out, "{{").ok();
    for ind in indicators {
        writeln!(out, "   if({} != INVALID_HANDLE) IndicatorRelease({});", ind.handle_name, ind.handle_name).ok();
    }
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_on_tick(out: &mut String, strategy: &Strategy) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void OnTick()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   // Wait for new bar").ok();
    writeln!(out, "   static datetime prevBarTime = 0;").ok();
    writeln!(out, "   datetime currentBarTime = iTime(_Symbol, PERIOD_CURRENT, 0);").ok();
    writeln!(out, "   if(currentBarTime == prevBarTime) return;").ok();
    writeln!(out, "   prevBarTime = currentBarTime;").ok();
    writeln!(out).ok();

    // Daily trade counter reset
    if strategy.max_daily_trades.is_some() {
        writeln!(out, "   // Reset daily trade counter").ok();
        writeln!(out, "   MqlDateTime dt;").ok();
        writeln!(out, "   TimeCurrent(dt);").ok();
        writeln!(out, "   datetime today = StringToTime(StringFormat(\"%04d.%02d.%02d\", dt.year, dt.mon, dt.day));").ok();
        writeln!(out, "   if(today != lastTradeDay) {{ lastTradeDay = today; dailyTradeCount = 0; }}").ok();
        writeln!(out).ok();
    }

    // Trading hours check
    if strategy.trading_hours.is_some() {
        writeln!(out, "   // Trading hours filter").ok();
        if strategy.max_daily_trades.is_none() {
            writeln!(out, "   MqlDateTime dt;").ok();
            writeln!(out, "   TimeCurrent(dt);").ok();
        }
        writeln!(out, "   int currentMinutes = dt.hour * 60 + dt.min;").ok();
        writeln!(out, "   int startMinutes = InpStartHour * 60 + InpStartMinute;").ok();
        writeln!(out, "   int endMinutes = InpEndHour * 60 + InpEndMinute;").ok();
        writeln!(out, "   bool inHours;").ok();
        writeln!(out, "   if(startMinutes <= endMinutes)").ok();
        writeln!(out, "      inHours = (currentMinutes >= startMinutes && currentMinutes <= endMinutes);").ok();
        writeln!(out, "   else // crosses midnight").ok();
        writeln!(out, "      inHours = (currentMinutes >= startMinutes || currentMinutes <= endMinutes);").ok();
        writeln!(out).ok();
    }

    // Close trades at specific time
    if let Some(ct) = &strategy.close_trades_at {
        writeln!(out, "   // Force close at {}:{:02}", ct.hour, ct.minute).ok();
        if strategy.trading_hours.is_none() && strategy.max_daily_trades.is_none() {
            writeln!(out, "   MqlDateTime dt;").ok();
            writeln!(out, "   TimeCurrent(dt);").ok();
        }
        writeln!(out, "   if(dt.hour == {} && dt.min == {}) {{ CloseAllPositions(); return; }}", ct.hour, ct.minute).ok();
        writeln!(out).ok();
    }

    // Position check
    writeln!(out, "   bool hasPosition = PositionSelect(_Symbol);").ok();
    writeln!(out).ok();

    // Entry logic
    writeln!(out, "   if(!hasPosition)").ok();
    writeln!(out, "   {{").ok();

    let mut entry_conditions = Vec::new();
    if strategy.trading_hours.is_some() {
        entry_conditions.push("inHours".to_string());
    }
    if strategy.max_daily_trades.is_some() {
        entry_conditions.push("dailyTradeCount < InpMaxDailyTrades".to_string());
    }
    let guard = if entry_conditions.is_empty() {
        String::new()
    } else {
        format!("if({}) ", entry_conditions.join(" && "))
    };

    let can_long = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;

    if can_long && !strategy.long_entry_rules.is_empty() {
        writeln!(out, "      {}if(CheckLongEntry())", guard).ok();
        writeln!(out, "         OpenLong();").ok();
    } else if can_long {
        writeln!(out, "      // WARNING: No long entry rules defined").ok();
    }

    if can_short && !strategy.short_entry_rules.is_empty() {
        let kw = if can_long && !strategy.long_entry_rules.is_empty() { "else if" } else { "if" };
        writeln!(out, "      {}{}(CheckShortEntry())", if guard.is_empty() { "" } else { "else " }, kw).ok();
        // Fix: when there's a guard, we need it on the else if too
        if !guard.is_empty() && can_long && !strategy.long_entry_rules.is_empty() {
            // Already guarded by the outer if
        }
        writeln!(out, "         OpenShort();").ok();
    } else if can_short {
        writeln!(out, "      // WARNING: No short entry rules defined").ok();
    }

    writeln!(out, "   }}").ok();

    // Exit logic
    writeln!(out, "   else").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      long posType = PositionGetInteger(POSITION_TYPE);").ok();
    if can_long {
        if !strategy.long_exit_rules.is_empty() {
            writeln!(out, "      if(posType == POSITION_TYPE_BUY && CheckLongExit())").ok();
            writeln!(out, "         ClosePosition();").ok();
        }
    }
    if can_short {
        let kw = if can_long && !strategy.long_exit_rules.is_empty() { "else if" } else { "if" };
        if !strategy.short_exit_rules.is_empty() {
            writeln!(out, "      {}(posType == POSITION_TYPE_SELL && CheckShortExit())", kw).ok();
            writeln!(out, "         ClosePosition();").ok();
        }
    }
    if strategy.trailing_stop.is_some() {
        writeln!(out, "      ManageTrailingStop();").ok();
    }
    writeln!(out, "   }}").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_check_rules_fn(out: &mut String, rules: &[Rule], fn_name: &str, indicators: &[UniqueIndicator]) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "bool {}()", fn_name).ok();
    writeln!(out, "{{").ok();

    if rules.is_empty() {
        writeln!(out, "   // WARNING: No rules defined — always returns false").ok();
        writeln!(out, "   return false;").ok();
        writeln!(out, "}}").ok();
        writeln!(out).ok();
        return;
    }

    // Declare and copy buffers for needed indicators
    let needed = collect_indicators_from_rules(rules);
    for ind_key in &needed {
        if let Some(ind) = indicators.iter().find(|i| i.config.cache_key() == *ind_key) {
            // Need to determine which buffers are used
            let buffers_used = collect_buffers_used(rules, ind);
            for buf_idx in buffers_used {
                let suffix = buffer_suffix(ind.config.indicator_type, buf_idx);
                writeln!(out, "   double {}{}[];", ind.var_name, suffix).ok();
                writeln!(out, "   ArraySetAsSeries({}{}, true);", ind.var_name, suffix).ok();
                writeln!(out, "   if(CopyBuffer({}, {}, 0, 3, {}{}) < 3) return false;",
                    ind.handle_name, buf_idx, ind.var_name, suffix).ok();
            }
        }
    }
    writeln!(out).ok();

    // Build rule expressions
    for (i, rule) in rules.iter().enumerate() {
        let left_curr = mql5_operand_expr(&rule.left_operand, 0, indicators);
        let right_curr = mql5_operand_expr(&rule.right_operand, 0, indicators);

        let expr = match rule.comparator {
            Comparator::GreaterThan => format!("{} > {}", left_curr, right_curr),
            Comparator::LessThan => format!("{} < {}", left_curr, right_curr),
            Comparator::GreaterOrEqual => format!("{} >= {}", left_curr, right_curr),
            Comparator::LessOrEqual => format!("{} <= {}", left_curr, right_curr),
            Comparator::Equal => format!("{} == {}", left_curr, right_curr),
            Comparator::CrossAbove => {
                let left_prev = mql5_operand_expr(&rule.left_operand, 1, indicators);
                let right_prev = mql5_operand_expr(&rule.right_operand, 1, indicators);
                format!("({} <= {} && {} > {})", left_prev, right_prev, left_curr, right_curr)
            }
            Comparator::CrossBelow => {
                let left_prev = mql5_operand_expr(&rule.left_operand, 1, indicators);
                let right_prev = mql5_operand_expr(&rule.right_operand, 1, indicators);
                format!("({} >= {} && {} < {})", left_prev, right_prev, left_curr, right_curr)
            }
        };

        writeln!(out, "   bool rule{} = {};", i + 1, expr).ok();
    }
    writeln!(out).ok();

    // Combine with logical operators
    let mut combined = "rule1".to_string();
    for (i, _rule) in rules.iter().enumerate().skip(1) {
        let op = if let Some(LogicalOperator::Or) = rules[i - 1].logical_operator {
            "||"
        } else {
            "&&"
        };
        write!(combined, " {} rule{}", op, i + 1).ok();
    }

    writeln!(out, "   return {};", combined).ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn collect_indicators_from_rules(rules: &[Rule]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for rule in rules {
        for op in [&rule.left_operand, &rule.right_operand] {
            if op.operand_type == OperandType::Indicator {
                if let Some(ind) = &op.indicator {
                    let key = ind.cache_key();
                    if seen.insert(key.clone()) {
                        result.push(key);
                    }
                }
            }
        }
    }
    result
}

fn collect_buffers_used(rules: &[Rule], ind: &UniqueIndicator) -> Vec<usize> {
    let mut buffers = HashSet::new();
    let key = ind.config.cache_key();
    for rule in rules {
        for op in [&rule.left_operand, &rule.right_operand] {
            if let Some(cfg) = &op.indicator {
                if cfg.cache_key() == key {
                    buffers.insert(mql5_buffer_index(cfg));
                }
            }
        }
    }
    let mut v: Vec<usize> = buffers.into_iter().collect();
    v.sort();
    v
}

fn buffer_suffix(ind_type: IndicatorType, buf_idx: usize) -> &'static str {
    match ind_type {
        IndicatorType::MACD => match buf_idx { 1 => "_signal", 2 => "_hist", _ => "_line" },
        IndicatorType::BollingerBands => match buf_idx { 1 => "_upper", 2 => "_lower", _ => "_basis" },
        IndicatorType::Stochastic => match buf_idx { 1 => "_d", _ => "_k" },
        IndicatorType::ADX => match buf_idx { 1 => "_pdi", 2 => "_mdi", _ => "_val" },
        _ => "_buf",
    }
}

fn mql5_operand_expr(operand: &Operand, extra_shift: usize, indicators: &[UniqueIndicator]) -> String {
    let offset = operand.offset.unwrap_or(0) + extra_shift;

    match operand.operand_type {
        OperandType::Price => {
            let func = match operand.price_field.unwrap_or(PriceField::Close) {
                PriceField::Open => "iOpen",
                PriceField::High => "iHigh",
                PriceField::Low => "iLow",
                PriceField::Close => "iClose",
            };
            format!("{}(_Symbol, PERIOD_CURRENT, {})", func, offset)
        }
        OperandType::Constant => {
            let v = operand.constant_value.unwrap_or(0.0);
            if v == v.floor() && v.abs() < 1_000_000.0 {
                format!("{:.1}", v)
            } else {
                format!("{}", v)
            }
        }
        OperandType::Indicator => {
            if let Some(ind) = &operand.indicator {
                let key = ind.cache_key();
                if let Some(ui) = indicators.iter().find(|i| i.config.cache_key() == key) {
                    let buf_idx = mql5_buffer_index(ind);
                    let suffix = buffer_suffix(ui.config.indicator_type, buf_idx);
                    format!("{}{}[{}]", ui.var_name, suffix, offset)
                } else {
                    "0 /* indicator not found */".into()
                }
            } else {
                "0 /* no indicator config */".into()
            }
        }
    }
}

fn mql5_open_position(out: &mut String, direction: &str, order_type: &str, price_symbol: &str) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void Open{}()", direction).ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   double price = SymbolInfoDouble(_Symbol, {});", price_symbol).ok();
    writeln!(out, "   double sl = CalculateSL({}, price);", order_type).ok();
    writeln!(out, "   double lots = CalculateLotSize(price, sl);").ok();
    writeln!(out, "   double tp = CalculateTP({}, price, sl);", order_type).ok();
    writeln!(out).ok();
    writeln!(out, "   trade.PositionOpen(_Symbol, {}, lots, price, sl, tp, \"{} Entry\");", order_type, direction).ok();
    writeln!(out, "   dailyTradeCount++;").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_close_position(out: &mut String) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void ClosePosition()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   trade.PositionClose(_Symbol);").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
    writeln!(out, "void CloseAllPositions()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   for(int i = PositionsTotal() - 1; i >= 0; i--)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      if(PositionGetSymbol(i) == _Symbol && PositionGetInteger(POSITION_MAGIC) == InpMagicNumber)").ok();
    writeln!(out, "         trade.PositionClose(PositionGetTicket(i));").ok();
    writeln!(out, "   }}").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_lot_size(out: &mut String, strategy: &Strategy) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "double CalculateLotSize(double price, double sl)").ok();
    writeln!(out, "{{").ok();

    match strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots => {
            writeln!(out, "   return InpLotSize;").ok();
        }
        PositionSizingType::FixedAmount => {
            writeln!(out, "   // Fixed Amount: risk exactly $X per trade based on SL distance").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue == 0 || tickSize == 0) return SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN);").ok();
            writeln!(out, "   double slDistance = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   double lots = InpFixedAmount / slMoneyPerLot;").ok();
            writeln!(out, "   return NormalizeDouble(MathMax(lots, SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN)), 2);").ok();
        }
        PositionSizingType::PercentEquity => {
            writeln!(out, "   // Percent Equity: risk equity*X% per trade based on SL distance").ok();
            writeln!(out, "   double equity = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue == 0 || tickSize == 0) return SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN);").ok();
            writeln!(out, "   double riskAmount = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slDistance = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   double lots = riskAmount / slMoneyPerLot;").ok();
            writeln!(out, "   return NormalizeDouble(MathMax(lots, SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN)), 2);").ok();
        }
        PositionSizingType::RiskBased => {
            writeln!(out, "   // Risk-based: risk equity*X% per trade based on SL distance").ok();
            writeln!(out, "   double equity = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double riskAmount = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue == 0 || tickSize == 0) return SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN);").ok();
            writeln!(out, "   double slDistance = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   double lots = riskAmount / slMoneyPerLot;").ok();
            writeln!(out, "   return NormalizeDouble(MathMax(lots, SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN)), 2);").ok();
        }
    }

    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_sl_tp_helpers(out: &mut String, strategy: &Strategy) {
    // SL helper
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "double CalculateSL(ENUM_ORDER_TYPE orderType, double price)").ok();
    writeln!(out, "{{").ok();

    if let Some(sl) = &strategy.stop_loss {
        match sl.sl_type {
            StopLossType::Pips => {
                writeln!(out, "   double dist = InpSLPips * _Point * 10;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
            }
            StopLossType::Percentage => {
                writeln!(out, "   double dist = price * InpSLPct / 100.0;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
            }
            StopLossType::ATR => {
                let var = format!("atr_{}", sl.atr_period.unwrap_or(14));
                writeln!(out, "   double atrBuf[];").ok();
                writeln!(out, "   ArraySetAsSeries(atrBuf, true);").ok();
                writeln!(out, "   CopyBuffer(handle_{}, 0, 0, 1, atrBuf);", var).ok();
                writeln!(out, "   double dist = atrBuf[0] * InpSLAtrMult;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
            }
        }
    } else {
        writeln!(out, "   return 0; // No stop loss configured").ok();
    }

    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // TP helper
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "double CalculateTP(ENUM_ORDER_TYPE orderType, double price, double sl)").ok();
    writeln!(out, "{{").ok();

    if let Some(tp) = &strategy.take_profit {
        match tp.tp_type {
            TakeProfitType::Pips => {
                writeln!(out, "   double dist = InpTPPips * _Point * 10;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price + dist : price - dist;").ok();
            }
            TakeProfitType::RiskReward => {
                writeln!(out, "   double slDist = MathAbs(price - sl);").ok();
                writeln!(out, "   double tpDist = slDist * InpTPRR;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price + tpDist : price - tpDist;").ok();
            }
            TakeProfitType::ATR => {
                let var = format!("atr_{}", tp.atr_period.unwrap_or(14));
                writeln!(out, "   double atrBuf[];").ok();
                writeln!(out, "   ArraySetAsSeries(atrBuf, true);").ok();
                writeln!(out, "   CopyBuffer(handle_{}, 0, 0, 1, atrBuf);", var).ok();
                writeln!(out, "   double dist = atrBuf[0] * InpTPAtrMult;").ok();
                writeln!(out, "   return (orderType == ORDER_TYPE_BUY) ? price + dist : price - dist;").ok();
            }
        }
    } else {
        writeln!(out, "   return 0; // No take profit configured").ok();
    }

    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_trailing_stop(out: &mut String, strategy: &Strategy) {
    if strategy.trailing_stop.is_none() { return; }
    let ts = strategy.trailing_stop.as_ref().unwrap();

    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void ManageTrailingStop()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   if(!PositionSelect(_Symbol)) return;").ok();
    writeln!(out, "   double currentSL = PositionGetDouble(POSITION_SL);").ok();
    writeln!(out, "   double entryPrice = PositionGetDouble(POSITION_PRICE_OPEN);").ok();
    writeln!(out, "   long posType = PositionGetInteger(POSITION_TYPE);").ok();
    writeln!(out).ok();

    match ts.ts_type {
        TrailingStopType::ATR => {
            let var = format!("atr_{}", ts.atr_period.unwrap_or(14));
            writeln!(out, "   double atrBuf[];").ok();
            writeln!(out, "   ArraySetAsSeries(atrBuf, true);").ok();
            writeln!(out, "   CopyBuffer(handle_{}, 0, 0, 1, atrBuf);", var).ok();
            writeln!(out, "   double trailDist = atrBuf[0] * InpTSAtrMult;").ok();
        }
        TrailingStopType::RiskReward => {
            writeln!(out, "   double slDist = MathAbs(entryPrice - currentSL);").ok();
            writeln!(out, "   double trailDist = slDist * InpTSRR;").ok();
        }
    }

    writeln!(out).ok();
    writeln!(out, "   if(posType == POSITION_TYPE_BUY)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      double newSL = SymbolInfoDouble(_Symbol, SYMBOL_BID) - trailDist;").ok();
    writeln!(out, "      if(newSL > currentSL && newSL > entryPrice)").ok();
    writeln!(out, "         trade.PositionModify(_Symbol, newSL, PositionGetDouble(POSITION_TP));").ok();
    writeln!(out, "   }}").ok();
    writeln!(out, "   else").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      double newSL = SymbolInfoDouble(_Symbol, SYMBOL_ASK) + trailDist;").ok();
    writeln!(out, "      if(newSL < currentSL && newSL < entryPrice)").ok();
    writeln!(out, "         trade.PositionModify(_Symbol, newSL, PositionGetDouble(POSITION_TP));").ok();
    writeln!(out, "   }}").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

// ══════════════════════════════════════════════════════════════
// PineScript Generation
// ══════════════════════════════════════════════════════════════

fn pine_header(out: &mut String, strategy: &Strategy) {
    writeln!(out, "//@version=6").ok();

    // Build strategy() declaration
    let mut params = vec![
        format!("\"{}\"", strategy.name),
        "overlay=true".into(),
    ];

    // Initial capital (not in strategy model, use a sensible default)
    params.push("initial_capital=10000".into());

    // PineScript v6: explicit margin defaults
    params.push("margin_long=100".into());
    params.push("margin_short=100".into());

    // Position sizing
    match strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots => {
            params.push(format!("default_qty_type=strategy.fixed, default_qty_value={:.2}", strategy.position_sizing.value));
        }
        PositionSizingType::PercentEquity => {
            params.push(format!("default_qty_type=strategy.percent_of_equity, default_qty_value={:.1}", strategy.position_sizing.value));
        }
        _ => {
            params.push(format!("default_qty_type=strategy.fixed, default_qty_value={:.2}", strategy.position_sizing.value));
        }
    }

    // Commission
    if strategy.trading_costs.commission_value > 0.0 {
        match strategy.trading_costs.commission_type {
            CommissionType::Percentage => {
                params.push(format!("commission_type=strategy.commission.percent, commission_value={:.3}", strategy.trading_costs.commission_value));
            }
            CommissionType::FixedPerLot => {
                params.push(format!("commission_type=strategy.commission.cash_per_contract, commission_value={:.2}", strategy.trading_costs.commission_value));
            }
        }
    }

    // Slippage
    if strategy.trading_costs.slippage_pips > 0.0 {
        writeln!(out, "// NOTE: Slippage of {:.1} pips configured — PineScript has limited slippage support", strategy.trading_costs.slippage_pips).ok();
        params.push(format!("slippage={}", (strategy.trading_costs.slippage_pips * 10.0) as i64));
    }

    writeln!(out, "strategy({})", params.join(", ")).ok();
    writeln!(out).ok();
}

fn pine_inputs(out: &mut String, strategy: &Strategy, indicators: &[UniqueIndicator]) {
    writeln!(out, "// ═══════════════ INPUTS ═══════════════").ok();

    for ind in indicators {
        let p = &ind.config.params;
        match ind.config.indicator_type {
            IndicatorType::SMA | IndicatorType::EMA | IndicatorType::RSI |
            IndicatorType::ATR | IndicatorType::ADX | IndicatorType::CCI |
            IndicatorType::ROC | IndicatorType::WilliamsR => {
                if let Some(period) = p.period {
                    writeln!(out, "i_{}_period = input.int({}, \"{}\")", ind.var_name, period,
                        format!("{:?} Period", ind.config.indicator_type)).ok();
                }
            }
            IndicatorType::MACD => {
                writeln!(out, "i_{}_fast = input.int({}, \"MACD Fast\")", ind.var_name, p.fast_period.unwrap_or(12)).ok();
                writeln!(out, "i_{}_slow = input.int({}, \"MACD Slow\")", ind.var_name, p.slow_period.unwrap_or(26)).ok();
                writeln!(out, "i_{}_signal = input.int({}, \"MACD Signal\")", ind.var_name, p.signal_period.unwrap_or(9)).ok();
            }
            IndicatorType::BollingerBands => {
                writeln!(out, "i_{}_period = input.int({}, \"BB Period\")", ind.var_name, p.period.unwrap_or(20)).ok();
                writeln!(out, "i_{}_stddev = input.float({:.1}, \"BB StdDev\")", ind.var_name, p.std_dev.unwrap_or(2.0)).ok();
            }
            IndicatorType::Stochastic => {
                writeln!(out, "i_{}_k = input.int({}, \"Stoch K\")", ind.var_name, p.k_period.unwrap_or(14)).ok();
                writeln!(out, "i_{}_d = input.int({}, \"Stoch D\")", ind.var_name, p.d_period.unwrap_or(3)).ok();
            }
            IndicatorType::ParabolicSAR => {
                writeln!(out, "i_{}_af = input.float({:.2}, \"SAR Accel\")", ind.var_name, p.acceleration_factor.unwrap_or(0.02)).ok();
                writeln!(out, "i_{}_max = input.float({:.2}, \"SAR Max\")", ind.var_name, p.maximum_factor.unwrap_or(0.20)).ok();
            }
            IndicatorType::VWAP => {} // no params
        }
    }

    // SL/TP inputs
    if let Some(sl) = &strategy.stop_loss {
        match sl.sl_type {
            StopLossType::Pips => writeln!(out, "i_sl_pips = input.float({:.1}, \"Stop Loss (pips)\")", sl.value).ok(),
            StopLossType::Percentage => writeln!(out, "i_sl_pct = input.float({:.2}, \"Stop Loss (%)\")", sl.value).ok(),
            StopLossType::ATR => writeln!(out, "i_sl_atr_mult = input.float({:.1}, \"SL ATR Multiplier\")", sl.value).ok(),
        };
    }
    if let Some(tp) = &strategy.take_profit {
        match tp.tp_type {
            TakeProfitType::Pips => writeln!(out, "i_tp_pips = input.float({:.1}, \"Take Profit (pips)\")", tp.value).ok(),
            TakeProfitType::RiskReward => writeln!(out, "i_tp_rr = input.float({:.1}, \"TP Risk:Reward\")", tp.value).ok(),
            TakeProfitType::ATR => writeln!(out, "i_tp_atr_mult = input.float({:.1}, \"TP ATR Multiplier\")", tp.value).ok(),
        };
    }

    // Trailing stop inputs
    if let Some(ts) = &strategy.trailing_stop {
        match ts.ts_type {
            TrailingStopType::ATR => writeln!(out, "i_ts_atr_mult = input.float({:.1}, \"Trailing Stop ATR Multiplier\")", ts.value).ok(),
            TrailingStopType::RiskReward => writeln!(out, "i_ts_rr = input.float({:.1}, \"Trailing Stop R:R\")", ts.value).ok(),
        };
    }

    if let Some(th) = &strategy.trading_hours {
        writeln!(out, "i_start_hour = input.int({}, \"Start Hour\")", th.start_hour).ok();
        writeln!(out, "i_start_minute = input.int({}, \"Start Minute\")", th.start_minute).ok();
        writeln!(out, "i_end_hour = input.int({}, \"End Hour\")", th.end_hour).ok();
        writeln!(out, "i_end_minute = input.int({}, \"End Minute\")", th.end_minute).ok();
    }

    writeln!(out).ok();
}

fn pine_indicators(out: &mut String, indicators: &[UniqueIndicator]) {
    writeln!(out, "// ═══════════════ INDICATORS ═══════════════").ok();

    for ind in indicators {
        match ind.config.indicator_type {
            IndicatorType::SMA => {
                writeln!(out, "{} = ta.sma(close, i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::EMA => {
                writeln!(out, "{} = ta.ema(close, i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::RSI => {
                writeln!(out, "{} = ta.rsi(close, i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::MACD => {
                writeln!(out, "[{0}_line, {0}_signal, {0}_hist] = ta.macd(close, i_{0}_fast, i_{0}_slow, i_{0}_signal)", ind.var_name).ok();
            }
            IndicatorType::BollingerBands => {
                writeln!(out, "[{0}_basis, {0}_upper, {0}_lower] = ta.bb(close, i_{0}_period, i_{0}_stddev)", ind.var_name).ok();
            }
            IndicatorType::ATR => {
                writeln!(out, "{} = ta.atr(i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::Stochastic => {
                writeln!(out, "{0}_k = ta.stoch(close, high, low, i_{0}_k)", ind.var_name).ok();
                writeln!(out, "{0}_d = ta.sma({0}_k, i_{0}_d)", ind.var_name).ok();
            }
            IndicatorType::ADX => {
                writeln!(out, "[{0}_pdi, {0}_mdi, {0}_val] = ta.dmi(i_{0}_period, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::CCI => {
                writeln!(out, "{} = ta.cci(close, i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::ROC => {
                writeln!(out, "{} = ta.roc(close, i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::WilliamsR => {
                writeln!(out, "{} = ta.wpr(i_{}_period)", ind.var_name, ind.var_name).ok();
            }
            IndicatorType::ParabolicSAR => {
                writeln!(out, "{} = ta.sar(i_{0}_af, i_{0}_af, i_{0}_max)", ind.var_name).ok();
            }
            IndicatorType::VWAP => {
                writeln!(out, "{} = ta.vwap(hlc3)", ind.var_name).ok();
            }
        }
    }

    writeln!(out).ok();
}

fn pine_trading_hours(out: &mut String, strategy: &Strategy) {
    if strategy.trading_hours.is_none() && strategy.close_trades_at.is_none() {
        return;
    }

    writeln!(out, "// ═══════════════ TIME FILTERS ═══════════════").ok();

    if strategy.trading_hours.is_some() {
        writeln!(out, "currentMinutes = hour * 60 + minute").ok();
        writeln!(out, "startMinutes = i_start_hour * 60 + i_start_minute").ok();
        writeln!(out, "endMinutes = i_end_hour * 60 + i_end_minute").ok();
        writeln!(out, "inTradingHours = startMinutes <= endMinutes ? (currentMinutes >= startMinutes and currentMinutes <= endMinutes) : (currentMinutes >= startMinutes or currentMinutes <= endMinutes)").ok();
    }

    if let Some(ct) = &strategy.close_trades_at {
        writeln!(out, "forceCloseTime = hour == {} and minute == {}", ct.hour, ct.minute).ok();
        writeln!(out, "if forceCloseTime and strategy.position_size != 0").ok();
        writeln!(out, "    strategy.close_all(comment=\"Time Close\")").ok();
    }

    writeln!(out).ok();
}

fn pine_conditions(out: &mut String, strategy: &Strategy) {
    writeln!(out, "// ═══════════════ ENTRY CONDITIONS ═══════════════").ok();

    let can_long = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;
    let has_hours = strategy.trading_hours.is_some();

    // Long entry
    if can_long {
        if strategy.long_entry_rules.is_empty() {
            writeln!(out, "longEntry = false // WARNING: No long entry rules defined").ok();
        } else {
            let expr = pine_rules_expression(&strategy.long_entry_rules);
            if has_hours {
                writeln!(out, "longEntry = ({}) and inTradingHours", expr).ok();
            } else {
                writeln!(out, "longEntry = {}", expr).ok();
            }
        }
    }

    // Short entry
    if can_short {
        if strategy.short_entry_rules.is_empty() {
            writeln!(out, "shortEntry = false // WARNING: No short entry rules defined").ok();
        } else {
            let expr = pine_rules_expression(&strategy.short_entry_rules);
            if has_hours {
                writeln!(out, "shortEntry = ({}) and inTradingHours", expr).ok();
            } else {
                writeln!(out, "shortEntry = {}", expr).ok();
            }
        }
    }

    writeln!(out).ok();
    writeln!(out, "// ═══════════════ EXIT CONDITIONS ═══════════════").ok();

    if can_long {
        if strategy.long_exit_rules.is_empty() {
            writeln!(out, "longExit = false // No long exit rules — relying on SL/TP").ok();
        } else {
            writeln!(out, "longExit = {}", pine_rules_expression(&strategy.long_exit_rules)).ok();
        }
    }
    if can_short {
        if strategy.short_exit_rules.is_empty() {
            writeln!(out, "shortExit = false // No short exit rules — relying on SL/TP").ok();
        } else {
            writeln!(out, "shortExit = {}", pine_rules_expression(&strategy.short_exit_rules)).ok();
        }
    }

    writeln!(out).ok();
}

fn pine_rules_expression(rules: &[Rule]) -> String {
    if rules.is_empty() {
        return "false".into();
    }

    let mut parts = Vec::new();
    for rule in rules {
        let left = pine_operand_expr(&rule.left_operand, 0);
        let right = pine_operand_expr(&rule.right_operand, 0);

        let expr = match rule.comparator {
            Comparator::GreaterThan => format!("{} > {}", left, right),
            Comparator::LessThan => format!("{} < {}", left, right),
            Comparator::GreaterOrEqual => format!("{} >= {}", left, right),
            Comparator::LessOrEqual => format!("{} <= {}", left, right),
            Comparator::Equal => format!("{} == {}", left, right),
            Comparator::CrossAbove => format!("ta.crossover({}, {})", left, right),
            Comparator::CrossBelow => format!("ta.crossunder({}, {})", left, right),
        };
        parts.push(expr);
    }

    // Chain with logical operators
    let mut result = parts[0].clone();
    for (i, part) in parts.iter().enumerate().skip(1) {
        let op = if let Some(LogicalOperator::Or) = rules[i - 1].logical_operator {
            "or"
        } else {
            "and"
        };
        write!(result, " {} {}", op, part).ok();
    }

    result
}

fn pine_operand_expr(operand: &Operand, extra_offset: usize) -> String {
    let offset = operand.offset.unwrap_or(0) + extra_offset;
    let offset_str = if offset > 0 { format!("[{}]", offset) } else { String::new() };

    match operand.operand_type {
        OperandType::Price => {
            let field = match operand.price_field.unwrap_or(PriceField::Close) {
                PriceField::Open => "open",
                PriceField::High => "high",
                PriceField::Low => "low",
                PriceField::Close => "close",
            };
            format!("{}{}", field, offset_str)
        }
        OperandType::Constant => {
            let v = operand.constant_value.unwrap_or(0.0);
            if v == v.floor() && v.abs() < 1_000_000.0 {
                format!("{:.1}", v)
            } else {
                format!("{}", v)
            }
        }
        OperandType::Indicator => {
            if let Some(ind) = &operand.indicator {
                let var = indicator_var_name(ind);
                let suffix = if is_multi_output(ind.indicator_type) {
                    pine_output_suffix(ind).to_string()
                } else {
                    String::new()
                };
                format!("{}{}{}", var, suffix, offset_str)
            } else {
                "na".into()
            }
        }
    }
}

fn pine_execution(out: &mut String, strategy: &Strategy) {
    writeln!(out, "// ═══════════════ EXECUTION ═══════════════").ok();

    let can_long = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;

    if can_long {
        writeln!(out, "if longEntry and strategy.position_size == 0").ok();
        writeln!(out, "    strategy.entry(\"Long\", strategy.long)").ok();
        writeln!(out).ok();
    }
    if can_short {
        writeln!(out, "if shortEntry and strategy.position_size == 0").ok();
        writeln!(out, "    strategy.entry(\"Short\", strategy.short)").ok();
        writeln!(out).ok();
    }
    if can_long {
        writeln!(out, "if strategy.position_size > 0 and longExit").ok();
        writeln!(out, "    strategy.close(\"Long\", comment=\"Exit Signal\")").ok();
        writeln!(out).ok();
    }
    if can_short {
        writeln!(out, "if strategy.position_size < 0 and shortExit").ok();
        writeln!(out, "    strategy.close(\"Short\", comment=\"Exit Signal\")").ok();
        writeln!(out).ok();
    }
}

fn pine_sl_tp(out: &mut String, strategy: &Strategy) {
    if strategy.stop_loss.is_none() && strategy.take_profit.is_none() && strategy.trailing_stop.is_none() {
        return;
    }

    writeln!(out, "// ═══════════════ STOP LOSS / TAKE PROFIT ═══════════════").ok();

    let can_long = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;

    // Calculate SL distance
    if let Some(sl) = &strategy.stop_loss {
        match sl.sl_type {
            StopLossType::Pips => {
                writeln!(out, "slDist = i_sl_pips * syminfo.mintick * 10").ok();
            }
            StopLossType::Percentage => {
                writeln!(out, "slDist = close * i_sl_pct / 100.0").ok();
            }
            StopLossType::ATR => {
                let var = format!("atr_{}", sl.atr_period.unwrap_or(14));
                writeln!(out, "slDist = {} * i_sl_atr_mult", var).ok();
            }
        }
    }

    // Calculate TP distance
    if let Some(tp) = &strategy.take_profit {
        match tp.tp_type {
            TakeProfitType::Pips => {
                writeln!(out, "tpDist = i_tp_pips * syminfo.mintick * 10").ok();
            }
            TakeProfitType::RiskReward => {
                if strategy.stop_loss.is_some() {
                    writeln!(out, "tpDist = slDist * i_tp_rr").ok();
                } else {
                    writeln!(out, "tpDist = close * 0.02 // NOTE: No SL defined for R:R calculation").ok();
                }
            }
            TakeProfitType::ATR => {
                let var = format!("atr_{}", tp.atr_period.unwrap_or(14));
                writeln!(out, "tpDist = {} * i_tp_atr_mult", var).ok();
            }
        }
    }

    writeln!(out).ok();

    // strategy.exit calls
    if can_long {
        let mut exit_params = vec!["\"Long\"".to_string(), "from_entry=\"Long\"".to_string()];
        if strategy.stop_loss.is_some() {
            exit_params.push("stop=strategy.position_avg_price - slDist".into());
        }
        if strategy.take_profit.is_some() {
            exit_params.push("limit=strategy.position_avg_price + tpDist".into());
        }
        if let Some(ts) = &strategy.trailing_stop {
            match ts.ts_type {
                TrailingStopType::ATR => {
                    let var = format!("atr_{}", ts.atr_period.unwrap_or(14));
                    exit_params.push(format!("trail_points={} * i_ts_atr_mult / syminfo.mintick", var));
                    exit_params.push(format!("trail_offset={} * i_ts_atr_mult / syminfo.mintick", var));
                }
                TrailingStopType::RiskReward => {
                    if strategy.stop_loss.is_some() {
                        exit_params.push("trail_points=slDist / syminfo.mintick".into());
                        exit_params.push("trail_offset=slDist * i_ts_rr / syminfo.mintick".into());
                    }
                }
            }
        }
        writeln!(out, "strategy.exit({})", exit_params.join(", ")).ok();
    }

    if can_short {
        let mut exit_params = vec!["\"Short\"".to_string(), "from_entry=\"Short\"".to_string()];
        if strategy.stop_loss.is_some() {
            exit_params.push("stop=strategy.position_avg_price + slDist".into());
        }
        if strategy.take_profit.is_some() {
            exit_params.push("limit=strategy.position_avg_price - tpDist".into());
        }
        if let Some(ts) = &strategy.trailing_stop {
            match ts.ts_type {
                TrailingStopType::ATR => {
                    let var = format!("atr_{}", ts.atr_period.unwrap_or(14));
                    exit_params.push(format!("trail_points={} * i_ts_atr_mult / syminfo.mintick", var));
                    exit_params.push(format!("trail_offset={} * i_ts_atr_mult / syminfo.mintick", var));
                }
                TrailingStopType::RiskReward => {
                    if strategy.stop_loss.is_some() {
                        exit_params.push("trail_points=slDist / syminfo.mintick".into());
                        exit_params.push("trail_offset=slDist * i_ts_rr / syminfo.mintick".into());
                    }
                }
            }
        }
        writeln!(out, "strategy.exit({})", exit_params.join(", ")).ok();
    }

    writeln!(out).ok();
}

fn pine_plots(out: &mut String, indicators: &[UniqueIndicator], strategy: &Strategy) {
    writeln!(out, "// ═══════════════ VISUALIZATION ═══════════════").ok();

    for ind in indicators {
        match ind.config.indicator_type {
            IndicatorType::SMA => {
                writeln!(out, "plot({}, \"SMA\", color=color.blue, linewidth=1)", ind.var_name).ok();
            }
            IndicatorType::EMA => {
                writeln!(out, "plot({}, \"EMA\", color=color.orange, linewidth=1)", ind.var_name).ok();
            }
            IndicatorType::BollingerBands => {
                writeln!(out, "plot({}_upper, \"BB Upper\", color=color.gray)", ind.var_name).ok();
                writeln!(out, "plot({}_basis, \"BB Basis\", color=color.blue)", ind.var_name).ok();
                writeln!(out, "plot({}_lower, \"BB Lower\", color=color.gray)", ind.var_name).ok();
            }
            IndicatorType::ParabolicSAR => {
                writeln!(out, "plot({}, \"SAR\", style=plot.style_circles, color=color.purple, linewidth=1)", ind.var_name).ok();
            }
            IndicatorType::VWAP => {
                writeln!(out, "plot({}, \"VWAP\", color=color.yellow, linewidth=2)", ind.var_name).ok();
            }
            _ => {} // Non-overlay indicators (RSI, MACD, etc.) would need separate pane
        }
    }

    let can_long = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;

    writeln!(out).ok();
    writeln!(out, "// Plot entry signals").ok();
    if can_long {
        writeln!(out, "plotshape(longEntry, style=shape.triangleup, location=location.belowbar, color=color.green, size=size.small, title=\"Long Entry\")").ok();
    }
    if can_short {
        writeln!(out, "plotshape(shortEntry, style=shape.triangledown, location=location.abovebar, color=color.red, size=size.small, title=\"Short Entry\")").ok();
    }
}

// ══════════════════════════════════════════════════════════════
// Custom MQL5 Indicator Generation
// ══════════════════════════════════════════════════════════════
//
// Each function generates a standalone .mq5 indicator file that
// replicates the exact same calculation as our Rust engine.
// This ensures consistency between backtest and live MT5 trading.

fn generate_custom_indicator(ind_type: IndicatorType) -> Option<(String, String)> {
    let (filename, code) = match ind_type {
        IndicatorType::SMA => ("BT_SMA.mq5".into(), gen_mql5_sma()),
        IndicatorType::EMA => ("BT_EMA.mq5".into(), gen_mql5_ema()),
        IndicatorType::RSI => ("BT_RSI.mq5".into(), gen_mql5_rsi()),
        IndicatorType::MACD => ("BT_MACD.mq5".into(), gen_mql5_macd()),
        IndicatorType::BollingerBands => ("BT_BollingerBands.mq5".into(), gen_mql5_bollinger()),
        IndicatorType::ATR => ("BT_ATR.mq5".into(), gen_mql5_atr()),
        IndicatorType::Stochastic => ("BT_Stochastic.mq5".into(), gen_mql5_stochastic()),
        IndicatorType::ADX => ("BT_ADX.mq5".into(), gen_mql5_adx()),
        IndicatorType::CCI => ("BT_CCI.mq5".into(), gen_mql5_cci()),
        IndicatorType::ROC => ("BT_ROC.mq5".into(), gen_mql5_roc()),
        IndicatorType::WilliamsR => ("BT_WilliamsR.mq5".into(), gen_mql5_williams_r()),
        IndicatorType::ParabolicSAR => ("BT_ParabolicSAR.mq5".into(), gen_mql5_parabolic_sar()),
        IndicatorType::VWAP => ("BT_VWAP.mq5".into(), gen_mql5_vwap()),
    };
    Some((filename, code))
}

fn mql5_indicator_header(name: &str) -> String {
    format!(
r#"//+------------------------------------------------------------------+
//|                                                   {name}.mq5  |
//|              Custom indicator — Generated by Backtester Rust      |
//|   Replicates exact calculation from backtester engine.            |
//+------------------------------------------------------------------+
#property copyright "Generated by Backtester Rust"
#property version   "1.00"
#property strict
"#)
}

// ── BT_SMA ──

fn gen_mql5_sma() -> String {
    let mut out = mql5_indicator_header("BT_SMA");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "SMA"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1

input int InpPeriod = 14; // Period

double SmaBuffer[];

int OnInit()
{
   SetIndexBuffer(0, SmaBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_SMA(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   int start;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
         SmaBuffer[i] = EMPTY_VALUE;

      // First SMA: simple sum of first 'period' closes
      double sum = 0;
      for(int i = 0; i < InpPeriod; i++)
         sum += close[i];
      SmaBuffer[InpPeriod - 1] = sum / InpPeriod;
      start = InpPeriod;
   }
   else
   {
      start = prev_calculated - 1;
   }

   // Rolling SMA using add/subtract
   for(int i = start; i < rates_total; i++)
   {
      SmaBuffer[i] = SmaBuffer[i - 1] + (close[i] - close[i - InpPeriod]) / InpPeriod;
   }

   return rates_total;
}
"#);
    out
}

// ── BT_EMA ──

fn gen_mql5_ema() -> String {
    let mut out = mql5_indicator_header("BT_EMA");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "EMA"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrOrangeRed
#property indicator_width1  1

input int InpPeriod = 14; // Period

double EmaBuffer[];

int OnInit()
{
   SetIndexBuffer(0, EmaBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_EMA(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   double multiplier = 2.0 / (InpPeriod + 1.0);

   int start;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
         EmaBuffer[i] = EMPTY_VALUE;

      // Seed with SMA of first 'period' values
      double sum = 0;
      for(int i = 0; i < InpPeriod; i++)
         sum += close[i];
      EmaBuffer[InpPeriod - 1] = sum / InpPeriod;
      start = InpPeriod;
   }
   else
   {
      start = prev_calculated - 1;
   }

   for(int i = start; i < rates_total; i++)
   {
      EmaBuffer[i] = (close[i] - EmaBuffer[i - 1]) * multiplier + EmaBuffer[i - 1];
   }

   return rates_total;
}
"#);
    out
}

// ── BT_RSI ──

fn gen_mql5_rsi() -> String {
    let mut out = mql5_indicator_header("BT_RSI");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "RSI"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrMediumPurple
#property indicator_width1  1
#property indicator_level1  70
#property indicator_level2  30
#property indicator_minimum 0
#property indicator_maximum 100

input int InpPeriod = 14; // Period

double RsiBuffer[];

// Internal state
double gAvgGain = 0;
double gAvgLoss = 0;
bool   gSeeded  = false;

int OnInit()
{
   SetIndexBuffer(0, RsiBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod + 1);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_RSI(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod + 1) return 0;

   int start;
   if(prev_calculated == 0)
   {
      for(int i = 0; i <= InpPeriod; i++)
         RsiBuffer[i] = EMPTY_VALUE;

      // First average: simple average of first 'period' changes
      double sumGain = 0, sumLoss = 0;
      for(int i = 1; i <= InpPeriod; i++)
      {
         double change = close[i] - close[i - 1];
         if(change > 0) sumGain += change;
         else           sumLoss += (-change);
      }
      gAvgGain = sumGain / InpPeriod;
      gAvgLoss = sumLoss / InpPeriod;
      gSeeded = true;

      if(gAvgLoss == 0)
         RsiBuffer[InpPeriod] = 100.0;
      else
         RsiBuffer[InpPeriod] = 100.0 - 100.0 / (1.0 + gAvgGain / gAvgLoss);

      start = InpPeriod + 1;
   }
   else
   {
      start = prev_calculated - 1;
   }

   // Smoothed averages (Wilder's method)
   for(int i = start; i < rates_total; i++)
   {
      double change = close[i] - close[i - 1];
      double gain = (change > 0) ? change : 0;
      double loss = (change < 0) ? (-change) : 0;

      gAvgGain = (gAvgGain * (InpPeriod - 1) + gain) / InpPeriod;
      gAvgLoss = (gAvgLoss * (InpPeriod - 1) + loss) / InpPeriod;

      if(gAvgLoss == 0)
         RsiBuffer[i] = 100.0;
      else
         RsiBuffer[i] = 100.0 - 100.0 / (1.0 + gAvgGain / gAvgLoss);
   }

   return rates_total;
}
"#);
    out
}

// ── BT_MACD ──

fn gen_mql5_macd() -> String {
    let mut out = mql5_indicator_header("BT_MACD");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 5
#property indicator_plots   3
#property indicator_label1  "MACD"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_label2  "Signal"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrRed
#property indicator_width2  1
#property indicator_label3  "Histogram"
#property indicator_type3   DRAW_HISTOGRAM
#property indicator_color3  clrGray
#property indicator_width3  2

input int InpFastPeriod   = 12; // Fast EMA Period
input int InpSlowPeriod   = 26; // Slow EMA Period
input int InpSignalPeriod = 9;  // Signal EMA Period

double MacdBuffer[];
double SignalBuffer[];
double HistBuffer[];
double FastEmaBuffer[];
double SlowEmaBuffer[];

int OnInit()
{
   SetIndexBuffer(0, MacdBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, SignalBuffer, INDICATOR_DATA);
   SetIndexBuffer(2, HistBuffer, INDICATOR_DATA);
   SetIndexBuffer(3, FastEmaBuffer, INDICATOR_CALCULATIONS);
   SetIndexBuffer(4, SlowEmaBuffer, INDICATOR_CALCULATIONS);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpSlowPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   PlotIndexSetDouble(1, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   PlotIndexSetDouble(2, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME,
      "BT_MACD(" + IntegerToString(InpFastPeriod) + "," +
      IntegerToString(InpSlowPeriod) + "," + IntegerToString(InpSignalPeriod) + ")");
   return INIT_SUCCEEDED;
}

// Helper: compute EMA buffer (SMA-seeded, same as Rust engine)
void ComputeEMA(const double &src[], double &dst[], int period, int rates_total, int prev_calculated)
{
   double mult = 2.0 / (period + 1.0);
   int start;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < period - 1; i++)
         dst[i] = EMPTY_VALUE;
      double sum = 0;
      for(int i = 0; i < period; i++)
         sum += src[i];
      dst[period - 1] = sum / period;
      start = period;
   }
   else
   {
      start = prev_calculated - 1;
   }
   for(int i = start; i < rates_total; i++)
      dst[i] = (src[i] - dst[i - 1]) * mult + dst[i - 1];
}

// Helper: EMA on a buffer that may contain EMPTY_VALUE
void ComputeEMAOnSlice(const double &src[], double &dst[], int period, int rates_total, int prev_calculated)
{
   double mult = 2.0 / (period + 1.0);
   if(prev_calculated == 0)
   {
      // Find first window of 'period' consecutive valid values
      int seedStart = -1;
      for(int i = 0; i <= rates_total - period; i++)
      {
         bool allValid = true;
         for(int j = i; j < i + period; j++)
         {
            if(src[j] == EMPTY_VALUE) { allValid = false; break; }
         }
         if(allValid) { seedStart = i; break; }
      }
      if(seedStart < 0) { ArrayInitialize(dst, EMPTY_VALUE); return; }

      for(int i = 0; i < seedStart + period - 1; i++)
         dst[i] = EMPTY_VALUE;

      double sum = 0;
      for(int i = seedStart; i < seedStart + period; i++)
         sum += src[i];
      dst[seedStart + period - 1] = sum / period;

      for(int i = seedStart + period; i < rates_total; i++)
      {
         if(src[i] == EMPTY_VALUE) { dst[i] = dst[i - 1]; continue; }
         dst[i] = (src[i] - dst[i - 1]) * mult + dst[i - 1];
      }
   }
   else
   {
      int start = prev_calculated - 1;
      for(int i = start; i < rates_total; i++)
      {
         if(src[i] == EMPTY_VALUE) { dst[i] = dst[i - 1]; continue; }
         dst[i] = (src[i] - dst[i - 1]) * mult + dst[i - 1];
      }
   }
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpSlowPeriod) return 0;

   // Fast & slow EMA on close
   ComputeEMA(close, FastEmaBuffer, InpFastPeriod, rates_total, prev_calculated);
   ComputeEMA(close, SlowEmaBuffer, InpSlowPeriod, rates_total, prev_calculated);

   // MACD line = fast - slow
   int start = (prev_calculated == 0) ? 0 : prev_calculated - 1;
   for(int i = start; i < rates_total; i++)
   {
      if(FastEmaBuffer[i] == EMPTY_VALUE || SlowEmaBuffer[i] == EMPTY_VALUE)
         MacdBuffer[i] = EMPTY_VALUE;
      else
         MacdBuffer[i] = FastEmaBuffer[i] - SlowEmaBuffer[i];
   }

   // Signal line = EMA of MACD line
   ComputeEMAOnSlice(MacdBuffer, SignalBuffer, InpSignalPeriod, rates_total, prev_calculated);

   // Histogram = MACD - Signal
   for(int i = start; i < rates_total; i++)
   {
      if(MacdBuffer[i] == EMPTY_VALUE || SignalBuffer[i] == EMPTY_VALUE)
         HistBuffer[i] = EMPTY_VALUE;
      else
         HistBuffer[i] = MacdBuffer[i] - SignalBuffer[i];
   }

   return rates_total;
}
"#);
    out
}

// ── BT_BollingerBands ──

fn gen_mql5_bollinger() -> String {
    let mut out = mql5_indicator_header("BT_BollingerBands");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 3
#property indicator_plots   3
#property indicator_label1  "Middle"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_label2  "Upper"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrGray
#property indicator_width2  1
#property indicator_label3  "Lower"
#property indicator_type3   DRAW_LINE
#property indicator_color3  clrGray
#property indicator_width3  1

input int    InpPeriod = 20;  // Period
input double InpStdDev = 2.0; // Std Dev Multiplier

double MiddleBuffer[];
double UpperBuffer[];
double LowerBuffer[];

int OnInit()
{
   // Buffer order matches Rust: primary=middle(0), secondary=upper(1), tertiary=lower(2)
   SetIndexBuffer(0, MiddleBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, UpperBuffer, INDICATOR_DATA);
   SetIndexBuffer(2, LowerBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetInteger(2, PLOT_DRAW_BEGIN, InpPeriod);
   for(int i = 0; i < 3; i++)
      PlotIndexSetDouble(i, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME,
      "BT_BB(" + IntegerToString(InpPeriod) + "," + DoubleToString(InpStdDev, 1) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   int start = (prev_calculated == 0) ? InpPeriod - 1 : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
      {
         MiddleBuffer[i] = EMPTY_VALUE;
         UpperBuffer[i] = EMPTY_VALUE;
         LowerBuffer[i] = EMPTY_VALUE;
      }
   }

   for(int i = start; i < rates_total; i++)
   {
      // SMA (middle band)
      double sum = 0;
      for(int j = i - InpPeriod + 1; j <= i; j++)
         sum += close[j];
      double mean = sum / InpPeriod;
      MiddleBuffer[i] = mean;

      // Population standard deviation (matching Rust: divide by N, not N-1)
      double variance = 0;
      for(int j = i - InpPeriod + 1; j <= i; j++)
      {
         double diff = close[j] - mean;
         variance += diff * diff;
      }
      variance /= InpPeriod;
      double sd = MathSqrt(variance);

      UpperBuffer[i] = mean + InpStdDev * sd;
      LowerBuffer[i] = mean - InpStdDev * sd;
   }

   return rates_total;
}
"#);
    out
}

// ── BT_ATR ──

fn gen_mql5_atr() -> String {
    let mut out = mql5_indicator_header("BT_ATR");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "ATR"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1

input int InpPeriod = 14; // Period

double AtrBuffer[];

// Internal state for Wilder's smoothing
double gAtr = 0;
bool   gSeeded = false;

int OnInit()
{
   SetIndexBuffer(0, AtrBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_ATR(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   int start;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
         AtrBuffer[i] = EMPTY_VALUE;

      // Compute TR and first ATR as SMA of first 'period' TR values
      // TR[0] = high[0] - low[0]
      double sumTR = high[0] - low[0];
      for(int i = 1; i < InpPeriod; i++)
      {
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         sumTR += MathMax(hl, MathMax(hc, lc));
      }
      gAtr = sumTR / InpPeriod;
      AtrBuffer[InpPeriod - 1] = gAtr;
      gSeeded = true;
      start = InpPeriod;
   }
   else
   {
      start = prev_calculated - 1;
   }

   // Wilder's smoothing: ATR = (prev_ATR * (period-1) + TR) / period
   for(int i = start; i < rates_total; i++)
   {
      double hl = high[i] - low[i];
      double hc = MathAbs(high[i] - close[i - 1]);
      double lc = MathAbs(low[i] - close[i - 1]);
      double tr = MathMax(hl, MathMax(hc, lc));

      gAtr = (gAtr * (InpPeriod - 1) + tr) / InpPeriod;
      AtrBuffer[i] = gAtr;
   }

   return rates_total;
}
"#);
    out
}

// ── BT_Stochastic ──

fn gen_mql5_stochastic() -> String {
    let mut out = mql5_indicator_header("BT_Stochastic");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   2
#property indicator_label1  "%K"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_label2  "%D"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrRed
#property indicator_width2  1
#property indicator_level1  80
#property indicator_level2  20
#property indicator_minimum 0
#property indicator_maximum 100

input int InpKPeriod = 14; // %K Period
input int InpDPeriod = 3;  // %D Period (SMA of %K)

double KBuffer[];
double DBuffer[];

int OnInit()
{
   SetIndexBuffer(0, KBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, DBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpKPeriod);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpKPeriod + InpDPeriod - 1);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   PlotIndexSetDouble(1, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME,
      "BT_Stoch(" + IntegerToString(InpKPeriod) + "," + IntegerToString(InpDPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpKPeriod) return 0;

   int start = (prev_calculated == 0) ? InpKPeriod - 1 : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpKPeriod - 1; i++)
      {
         KBuffer[i] = EMPTY_VALUE;
         DBuffer[i] = EMPTY_VALUE;
      }
   }

   // Compute %K
   for(int i = start; i < rates_total; i++)
   {
      double highest = high[i];
      double lowest  = low[i];
      for(int j = i - InpKPeriod + 1; j < i; j++)
      {
         if(high[j] > highest) highest = high[j];
         if(low[j] < lowest)   lowest  = low[j];
      }
      double range = highest - lowest;
      KBuffer[i] = (range == 0) ? 50.0 : (close[i] - lowest) / range * 100.0;
   }

   // Compute %D = SMA of %K
   int dStart = (prev_calculated == 0) ? InpKPeriod + InpDPeriod - 2 : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = InpKPeriod - 1; i < InpKPeriod + InpDPeriod - 2 && i < rates_total; i++)
         DBuffer[i] = EMPTY_VALUE;
   }

   for(int i = dStart; i < rates_total; i++)
   {
      double sum = 0;
      bool valid = true;
      for(int j = i - InpDPeriod + 1; j <= i; j++)
      {
         if(KBuffer[j] == EMPTY_VALUE) { valid = false; break; }
         sum += KBuffer[j];
      }
      DBuffer[i] = valid ? sum / InpDPeriod : EMPTY_VALUE;
   }

   return rates_total;
}
"#);
    out
}

// ── BT_ADX ──

fn gen_mql5_adx() -> String {
    let mut out = mql5_indicator_header("BT_ADX");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "ADX"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_level1  25

input int InpPeriod = 14; // Period

double AdxBuffer[];

// Internal state for Wilder's smoothing
double gSmoothTR = 0;
double gSmoothPDM = 0;
double gSmoothMDM = 0;
double gAdx = 0;
bool   gSmoothed = false;
bool   gAdxSeeded = false;
int    gDxCount = 0;
double gDxSum = 0;

int OnInit()
{
   SetIndexBuffer(0, AdxBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod * 2);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_ADX(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod * 2 + 1) return 0;

   if(prev_calculated == 0)
   {
      ArrayInitialize(AdxBuffer, EMPTY_VALUE);

      // Initialize: sum first 'period' TR, +DM, -DM (indices 1..period)
      gSmoothTR = 0;
      gSmoothPDM = 0;
      gSmoothMDM = 0;
      for(int i = 1; i <= InpPeriod; i++)
      {
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         gSmoothTR += MathMax(hl, MathMax(hc, lc));

         double upMove   = high[i] - high[i - 1];
         double downMove = low[i - 1] - low[i];
         gSmoothPDM += (upMove > downMove && upMove > 0) ? upMove : 0;
         gSmoothMDM += (downMove > upMove && downMove > 0) ? downMove : 0;
      }
      gSmoothed = true;

      // Compute DX for indices period..2*period-1 and accumulate for ADX seed
      gDxSum = 0;
      gDxCount = 0;

      // First DX at index=period
      {
         double pdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothPDM / gSmoothTR;
         double mdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothMDM / gSmoothTR;
         double diSum = pdi + mdi;
         double dx = (diSum == 0) ? 0 : 100.0 * MathAbs(pdi - mdi) / diSum;
         gDxSum += dx;
         gDxCount++;
      }

      // Continue Wilder's smoothing and accumulate DX
      for(int i = InpPeriod + 1; i < InpPeriod * 2; i++)
      {
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         double tr = MathMax(hl, MathMax(hc, lc));

         double upMove   = high[i] - high[i - 1];
         double downMove = low[i - 1] - low[i];
         double pdm = (upMove > downMove && upMove > 0) ? upMove : 0;
         double mdm = (downMove > upMove && downMove > 0) ? downMove : 0;

         gSmoothTR  = gSmoothTR  - gSmoothTR  / InpPeriod + tr;
         gSmoothPDM = gSmoothPDM - gSmoothPDM / InpPeriod + pdm;
         gSmoothMDM = gSmoothMDM - gSmoothMDM / InpPeriod + mdm;

         double pdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothPDM / gSmoothTR;
         double mdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothMDM / gSmoothTR;
         double diSum = pdi + mdi;
         double dx = (diSum == 0) ? 0 : 100.0 * MathAbs(pdi - mdi) / diSum;
         gDxSum += dx;
         gDxCount++;
      }

      // ADX seed = average of first 'period' DX values
      // This corresponds to index 2*period - 1
      {
         int i = InpPeriod * 2 - 1;
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         double tr = MathMax(hl, MathMax(hc, lc));

         double upMove   = high[i] - high[i - 1];
         double downMove = low[i - 1] - low[i];
         double pdm = (upMove > downMove && upMove > 0) ? upMove : 0;
         double mdm = (downMove > upMove && downMove > 0) ? downMove : 0;

         gSmoothTR  = gSmoothTR  - gSmoothTR  / InpPeriod + tr;
         gSmoothPDM = gSmoothPDM - gSmoothPDM / InpPeriod + pdm;
         gSmoothMDM = gSmoothMDM - gSmoothMDM / InpPeriod + mdm;

         double pdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothPDM / gSmoothTR;
         double mdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothMDM / gSmoothTR;
         double diSum = pdi + mdi;
         double dx = (diSum == 0) ? 0 : 100.0 * MathAbs(pdi - mdi) / diSum;
         gDxSum += dx;
         gDxCount++;

         gAdx = gDxSum / gDxCount;
         AdxBuffer[i] = gAdx;
         gAdxSeeded = true;
      }

      // Continue from 2*period onwards
      for(int i = InpPeriod * 2; i < rates_total; i++)
      {
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         double tr = MathMax(hl, MathMax(hc, lc));

         double upMove   = high[i] - high[i - 1];
         double downMove = low[i - 1] - low[i];
         double pdm = (upMove > downMove && upMove > 0) ? upMove : 0;
         double mdm = (downMove > upMove && downMove > 0) ? downMove : 0;

         gSmoothTR  = gSmoothTR  - gSmoothTR  / InpPeriod + tr;
         gSmoothPDM = gSmoothPDM - gSmoothPDM / InpPeriod + pdm;
         gSmoothMDM = gSmoothMDM - gSmoothMDM / InpPeriod + mdm;

         double pdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothPDM / gSmoothTR;
         double mdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothMDM / gSmoothTR;
         double diSum = pdi + mdi;
         double dx = (diSum == 0) ? 0 : 100.0 * MathAbs(pdi - mdi) / diSum;

         gAdx = (gAdx * (InpPeriod - 1) + dx) / InpPeriod;
         AdxBuffer[i] = gAdx;
      }
   }
   else
   {
      int start = prev_calculated - 1;
      for(int i = start; i < rates_total; i++)
      {
         double hl = high[i] - low[i];
         double hc = MathAbs(high[i] - close[i - 1]);
         double lc = MathAbs(low[i] - close[i - 1]);
         double tr = MathMax(hl, MathMax(hc, lc));

         double upMove   = high[i] - high[i - 1];
         double downMove = low[i - 1] - low[i];
         double pdm = (upMove > downMove && upMove > 0) ? upMove : 0;
         double mdm = (downMove > upMove && downMove > 0) ? downMove : 0;

         gSmoothTR  = gSmoothTR  - gSmoothTR  / InpPeriod + tr;
         gSmoothPDM = gSmoothPDM - gSmoothPDM / InpPeriod + pdm;
         gSmoothMDM = gSmoothMDM - gSmoothMDM / InpPeriod + mdm;

         double pdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothPDM / gSmoothTR;
         double mdi = (gSmoothTR == 0) ? 0 : 100.0 * gSmoothMDM / gSmoothTR;
         double diSum = pdi + mdi;
         double dx = (diSum == 0) ? 0 : 100.0 * MathAbs(pdi - mdi) / diSum;

         gAdx = (gAdx * (InpPeriod - 1) + dx) / InpPeriod;
         AdxBuffer[i] = gAdx;
      }
   }

   return rates_total;
}
"#);
    out
}

// ── BT_CCI ──

fn gen_mql5_cci() -> String {
    let mut out = mql5_indicator_header("BT_CCI");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "CCI"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_level1  100
#property indicator_level2  -100

input int InpPeriod = 20; // Period

double CciBuffer[];

int OnInit()
{
   SetIndexBuffer(0, CciBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_CCI(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   int start = (prev_calculated == 0) ? InpPeriod - 1 : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
         CciBuffer[i] = EMPTY_VALUE;
   }

   for(int i = start; i < rates_total; i++)
   {
      // Typical price for current bar
      double tp_i = (high[i] + low[i] + close[i]) / 3.0;

      // SMA of typical prices over period
      double sum = 0;
      for(int j = i - InpPeriod + 1; j <= i; j++)
         sum += (high[j] + low[j] + close[j]) / 3.0;
      double mean = sum / InpPeriod;

      // Mean deviation
      double meanDev = 0;
      for(int j = i - InpPeriod + 1; j <= i; j++)
      {
         double tp_j = (high[j] + low[j] + close[j]) / 3.0;
         meanDev += MathAbs(tp_j - mean);
      }
      meanDev /= InpPeriod;

      CciBuffer[i] = (meanDev == 0) ? 0 : (tp_i - mean) / (0.015 * meanDev);
   }

   return rates_total;
}
"#);
    out
}

// ── BT_ROC ──

fn gen_mql5_roc() -> String {
    let mut out = mql5_indicator_header("BT_ROC");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "ROC"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_level1  0

input int InpPeriod = 12; // Period

double RocBuffer[];

int OnInit()
{
   SetIndexBuffer(0, RocBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod + 1);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_ROC(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod + 1) return 0;

   int start = (prev_calculated == 0) ? InpPeriod : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod; i++)
         RocBuffer[i] = EMPTY_VALUE;
   }

   // ROC = (close - close[n]) / close[n] * 100
   for(int i = start; i < rates_total; i++)
   {
      if(close[i - InpPeriod] != 0)
         RocBuffer[i] = (close[i] - close[i - InpPeriod]) / close[i - InpPeriod] * 100.0;
      else
         RocBuffer[i] = 0;
   }

   return rates_total;
}
"#);
    out
}

// ── BT_WilliamsR ──

fn gen_mql5_williams_r() -> String {
    let mut out = mql5_indicator_header("BT_WilliamsR");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Williams %R"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrLimeGreen
#property indicator_width1  1
#property indicator_level1  -20
#property indicator_level2  -80
#property indicator_minimum -100
#property indicator_maximum 0

input int InpPeriod = 14; // Period

double WprBuffer[];

int OnInit()
{
   SetIndexBuffer(0, WprBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_WPR(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < InpPeriod) return 0;

   int start = (prev_calculated == 0) ? InpPeriod - 1 : prev_calculated - 1;
   if(prev_calculated == 0)
   {
      for(int i = 0; i < InpPeriod - 1; i++)
         WprBuffer[i] = EMPTY_VALUE;
   }

   for(int i = start; i < rates_total; i++)
   {
      double highest = high[i];
      double lowest  = low[i];
      for(int j = i - InpPeriod + 1; j < i; j++)
      {
         if(high[j] > highest) highest = high[j];
         if(low[j] < lowest)   lowest  = low[j];
      }
      double range = highest - lowest;
      WprBuffer[i] = (range == 0) ? -50.0 : (highest - close[i]) / range * (-100.0);
   }

   return rates_total;
}
"#);
    out
}

// ── BT_ParabolicSAR ──

fn gen_mql5_parabolic_sar() -> String {
    let mut out = mql5_indicator_header("BT_ParabolicSAR");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "SAR"
#property indicator_type1   DRAW_ARROW
#property indicator_color1  clrMagenta
#property indicator_width1  1

input double InpAF  = 0.02; // Acceleration Factor
input double InpMax = 0.20; // Maximum AF

double SarBuffer[];

int OnInit()
{
   SetIndexBuffer(0, SarBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_ARROW, 159); // small dot
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME,
      "BT_SAR(" + DoubleToString(InpAF, 2) + "," + DoubleToString(InpMax, 2) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < 2) return 0;

   // Full recalc — Parabolic SAR is path-dependent
   if(prev_calculated == 0)
   {
      bool isLong = (high[1] > high[0]);
      double af  = InpAF;
      double ep  = isLong ? high[0] : low[0];
      double sar = isLong ? low[0]  : high[0];

      SarBuffer[0] = sar;

      for(int i = 1; i < rates_total; i++)
      {
         double prevSar = sar;
         sar = prevSar + af * (ep - prevSar);

         if(isLong)
         {
            if(i >= 2)
               sar = MathMin(sar, MathMin(low[i - 1], low[i - 2]));
            else
               sar = MathMin(sar, low[i - 1]);

            if(low[i] < sar)
            {
               isLong = false;
               sar = ep;
               ep = low[i];
               af = InpAF;
            }
            else
            {
               if(high[i] > ep)
               {
                  ep = high[i];
                  af = MathMin(af + InpAF, InpMax);
               }
            }
         }
         else
         {
            if(i >= 2)
               sar = MathMax(sar, MathMax(high[i - 1], high[i - 2]));
            else
               sar = MathMax(sar, high[i - 1]);

            if(high[i] > sar)
            {
               isLong = true;
               sar = ep;
               ep = high[i];
               af = InpAF;
            }
            else
            {
               if(low[i] < ep)
               {
                  ep = low[i];
                  af = MathMin(af + InpAF, InpMax);
               }
            }
         }

         SarBuffer[i] = sar;
      }
   }
   else
   {
      // For incremental updates, we must recalculate from scratch
      // because SAR is path-dependent. Mark for full recalc.
      // In practice, MetaTrader will call with prev_calculated=0 on history load.
      // For live bars, recalc the last bar only if state is tracked.
      // For simplicity and correctness, recalc all.
      return 0; // Forces full recalc next tick
   }

   return rates_total;
}
"#);
    out
}

// ── BT_VWAP ──

fn gen_mql5_vwap() -> String {
    let mut out = mql5_indicator_header("BT_VWAP");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "VWAP"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_width1  2

double VwapBuffer[];

// Internal state for daily reset
double gCumTPVol = 0;
double gCumVol   = 0;
int    gLastDay  = -1;

int OnInit()
{
   SetIndexBuffer(0, VwapBuffer, INDICATOR_DATA);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_VWAP");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   if(rates_total < 1) return 0;

   int start;
   if(prev_calculated == 0)
   {
      gCumTPVol = 0;
      gCumVol   = 0;
      gLastDay  = -1;
      start = 0;
   }
   else
   {
      start = prev_calculated - 1;
   }

   for(int i = start; i < rates_total; i++)
   {
      MqlDateTime dt;
      TimeToStruct(time[i], dt);
      int currentDay = dt.day_of_year;

      // Reset on new day
      if(currentDay != gLastDay)
      {
         gCumTPVol = 0;
         gCumVol   = 0;
         gLastDay  = currentDay;
      }

      double tp = (high[i] + low[i] + close[i]) / 3.0;
      // Use tick_volume as volume proxy (standard for forex in MT5)
      double vol = (double)tick_volume[i];
      gCumTPVol += tp * vol;
      gCumVol   += vol;

      VwapBuffer[i] = (gCumVol == 0) ? tp : gCumTPVol / gCumVol;
   }

   return rates_total;
}
"#);
    out
}

// ══════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_strategy() -> Strategy {
        Strategy {
            id: "test-1".into(),
            name: "SMA Cross Test".into(),
            created_at: "2024-01-01".into(),
            updated_at: "2024-01-01".into(),
            long_entry_rules: vec![
                Rule {
                    id: "r1".into(),
                    left_operand: Operand {
                        operand_type: OperandType::Price,
                        price_field: Some(PriceField::Close),
                        indicator: None,
                        constant_value: None,
                        offset: None,
                    },
                    comparator: Comparator::CrossAbove,
                    right_operand: Operand {
                        operand_type: OperandType::Indicator,
                        indicator: Some(IndicatorConfig {
                            indicator_type: IndicatorType::SMA,
                            params: IndicatorParams { period: Some(20), ..Default::default() },
                            output_field: None,
                        }),
                        price_field: None,
                        constant_value: None,
                        offset: None,
                    },
                    logical_operator: Some(LogicalOperator::And),
                },
                Rule {
                    id: "r2".into(),
                    left_operand: Operand {
                        operand_type: OperandType::Indicator,
                        indicator: Some(IndicatorConfig {
                            indicator_type: IndicatorType::RSI,
                            params: IndicatorParams { period: Some(14), ..Default::default() },
                            output_field: None,
                        }),
                        price_field: None,
                        constant_value: None,
                        offset: None,
                    },
                    comparator: Comparator::GreaterThan,
                    right_operand: Operand {
                        operand_type: OperandType::Constant,
                        constant_value: Some(50.0),
                        indicator: None,
                        price_field: None,
                        offset: None,
                    },
                    logical_operator: None,
                },
            ],
            short_entry_rules: vec![],
            long_exit_rules: vec![],
            short_exit_rules: vec![],
            position_sizing: PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: 0.1,
            },
            stop_loss: Some(StopLoss {
                sl_type: StopLossType::Pips,
                value: 50.0,
                atr_period: None,
            }),
            take_profit: Some(TakeProfit {
                tp_type: TakeProfitType::Pips,
                value: 100.0,
                atr_period: None,
            }),
            trailing_stop: None,
            trading_costs: TradingCosts {
                spread_pips: 2.0,
                commission_type: CommissionType::Percentage,
                commission_value: 0.1,
                slippage_pips: 0.0,
                slippage_random: false,
            },
            trade_direction: TradeDirection::Long,
            trading_hours: None,
            max_daily_trades: None,
            close_trades_at: None,
        }
    }

    /// Helper: get main file code from a CodeGenerationResult
    fn main_code(result: &CodeGenerationResult) -> &str {
        &result.files.iter().find(|f| f.is_main).unwrap().code
    }

    #[test]
    fn test_mql5_generation() {
        let strategy = simple_strategy();
        let result = generate_mql5(&strategy).unwrap();
        let code = main_code(&result);

        assert!(code.contains("SMA Cross Test.mq5"));
        assert!(code.contains("handle_sma_20"));
        assert!(code.contains("handle_rsi_14"));
        // Now uses iCustom instead of built-in handles
        assert!(code.contains("iCustom(_Symbol, PERIOD_CURRENT, \"BT_SMA\""));
        assert!(code.contains("iCustom(_Symbol, PERIOD_CURRENT, \"BT_RSI\""));
        assert!(code.contains("bool CheckLongEntry()"));
        assert!(code.contains("rule1 && rule2"));
        assert!(code.contains("InpSLPips"));
        assert!(code.contains("InpTPPips"));

        // Verify custom indicator files are generated
        let filenames: Vec<&str> = result.files.iter().map(|f| f.filename.as_str()).collect();
        assert!(filenames.iter().any(|f| *f == "BT_SMA.mq5"));
        assert!(filenames.iter().any(|f| *f == "BT_RSI.mq5"));
    }

    #[test]
    fn test_pinescript_generation() {
        let strategy = simple_strategy();
        let result = generate_pinescript(&strategy).unwrap();
        let code = main_code(&result);

        assert!(code.contains("//@version=6"));
        assert!(code.contains("strategy(\"SMA Cross Test\""));
        assert!(code.contains("ta.sma(close, i_sma_20_period)"));
        assert!(code.contains("ta.rsi(close, i_rsi_14_period)"));
        assert!(code.contains("ta.crossover(close, sma_20)"));
        assert!(code.contains("rsi_14 > 50.0"));
        assert!(code.contains("strategy.entry(\"Long\", strategy.long)"));
        // PineScript has no extra files
        assert_eq!(result.files.len(), 1);
    }

    #[test]
    fn test_empty_rules() {
        let mut strategy = simple_strategy();
        strategy.long_entry_rules.clear();

        let mql5 = generate_mql5(&strategy).unwrap();
        assert!(main_code(&mql5).contains("WARNING: No rules defined"));

        let pine = generate_pinescript(&strategy).unwrap();
        assert!(main_code(&pine).contains("WARNING: No long entry rules defined"));
    }

    #[test]
    fn test_macd_multi_output() {
        let mut strategy = simple_strategy();
        strategy.long_entry_rules = vec![Rule {
            id: "r1".into(),
            left_operand: Operand {
                operand_type: OperandType::Indicator,
                indicator: Some(IndicatorConfig {
                    indicator_type: IndicatorType::MACD,
                    params: IndicatorParams {
                        fast_period: Some(12),
                        slow_period: Some(26),
                        signal_period: Some(9),
                        ..Default::default()
                    },
                    output_field: Some("signal".into()),
                }),
                price_field: None,
                constant_value: None,
                offset: None,
            },
            comparator: Comparator::GreaterThan,
            right_operand: Operand {
                operand_type: OperandType::Constant,
                constant_value: Some(0.0),
                indicator: None,
                price_field: None,
                offset: None,
            },
            logical_operator: None,
        }];

        let result = generate_mql5(&strategy).unwrap();
        let code = main_code(&result);
        assert!(code.contains("iCustom(_Symbol, PERIOD_CURRENT, \"BT_MACD\""));
        assert!(code.contains("_signal"));
        // MACD custom indicator file should be generated
        assert!(result.files.iter().any(|f| f.filename == "BT_MACD.mq5"));

        let pine = generate_pinescript(&strategy).unwrap();
        assert!(main_code(&pine).contains("ta.macd"));
        assert!(main_code(&pine).contains("macd_f12_s26_sig9_signal"));
    }

    #[test]
    fn test_custom_indicator_files_content() {
        let strategy = simple_strategy();
        let result = generate_mql5(&strategy).unwrap();

        // Check SMA indicator file content
        let sma_file = result.files.iter().find(|f| f.filename == "BT_SMA.mq5").unwrap();
        assert!(sma_file.code.contains("OnCalculate"));
        assert!(sma_file.code.contains("InpPeriod"));
        assert!(sma_file.code.contains("SmaBuffer"));
        assert!(!sma_file.is_main);

        // Check RSI indicator file content
        let rsi_file = result.files.iter().find(|f| f.filename == "BT_RSI.mq5").unwrap();
        assert!(rsi_file.code.contains("OnCalculate"));
        assert!(rsi_file.code.contains("Wilder"));
    }
}
