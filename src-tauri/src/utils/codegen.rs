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
    mql5_check_rules_fn(&mut out, &strategy.long_entry_rules, &strategy.long_entry_groups, "CheckLongEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_entry_rules, &strategy.short_entry_groups, "CheckShortEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.long_exit_rules, &strategy.long_exit_groups, "CheckLongExit", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_exit_rules, &strategy.short_exit_groups, "CheckShortExit", &indicators);
    mql5_open_position(&mut out, "Long", "ORDER_TYPE_BUY", "SYMBOL_ASK");
    mql5_open_position(&mut out, "Short", "ORDER_TYPE_SELL", "SYMBOL_BID");
    mql5_close_position(&mut out);
    mql5_lot_size(&mut out, strategy);
    mql5_sl_tp_helpers(&mut out, strategy);
    mql5_trailing_stop(&mut out, strategy);
    mql5_time_helpers(&mut out, strategy);

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

    // Flat rules + all rules nested inside rule groups
    let flat_rules = strategy.long_entry_rules.iter()
        .chain(&strategy.short_entry_rules)
        .chain(&strategy.long_exit_rules)
        .chain(&strategy.short_exit_rules);

    let group_rules = strategy.long_entry_groups.iter()
        .chain(&strategy.short_entry_groups)
        .chain(&strategy.long_exit_groups)
        .chain(&strategy.short_exit_groups)
        .flat_map(|g| g.rules.iter());

    for rule in flat_rules.chain(group_rules) {
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
        cached_hash: 0,
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
        IndicatorType::Aroon => "aroon",
        IndicatorType::AwesomeOscillator => "ao",
        IndicatorType::BarRange => "barrange",
        IndicatorType::BiggestRange => "bigrange",
        IndicatorType::HighestInRange => "highest",
        IndicatorType::LowestInRange => "lowest",
        IndicatorType::SmallestRange => "smallrange",
        IndicatorType::BearsPower => "bears",
        IndicatorType::BullsPower => "bulls",
        IndicatorType::DeMarker => "demarker",
        IndicatorType::Fibonacci => "fibo",
        IndicatorType::Fractal => "fractal",
        IndicatorType::GannHiLo => "gannhilo",
        IndicatorType::HeikenAshi => "ha",
        IndicatorType::HullMA => "hma",
        IndicatorType::Ichimoku => "ichi",
        IndicatorType::KeltnerChannel => "kc",
        IndicatorType::LaguerreRSI => "lrsi",
        IndicatorType::LinearRegression => "linreg",
        IndicatorType::Momentum => "mom",
        IndicatorType::SuperTrend => "st",
        IndicatorType::TrueRange => "tr",
        IndicatorType::StdDev => "stddev",
        IndicatorType::Reflex => "reflex",
        IndicatorType::Pivots => "pivots",
        IndicatorType::UlcerIndex => "ulcer",
        IndicatorType::Vortex => "vortex",
    };

    let mut s = String::from(name);

    // These indicators have no parameters in their MQL5 implementation — don't
    // append any suffix so duplicates are deduplicated and no phantom Inp_* vars are needed.
    let no_params = matches!(ind.indicator_type,
        IndicatorType::BarRange | IndicatorType::TrueRange |
        IndicatorType::AwesomeOscillator | IndicatorType::VWAP |
        IndicatorType::Fractal | IndicatorType::HeikenAshi |
        IndicatorType::Pivots
    );
    if no_params { return s; }

    if let Some(p) = ind.params.period { write!(s, "_{}", p).ok(); }
    if let Some(p) = ind.params.fast_period { write!(s, "_f{}", p).ok(); }
    if let Some(p) = ind.params.slow_period { write!(s, "_s{}", p).ok(); }
    if let Some(p) = ind.params.signal_period { write!(s, "_sig{}", p).ok(); }
    if let Some(p) = ind.params.k_period { write!(s, "_k{}", p).ok(); }
    if let Some(p) = ind.params.d_period { write!(s, "_d{}", p).ok(); }
    if let Some(v) = ind.params.std_dev { write!(s, "_sd{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.acceleration_factor { write!(s, "_af{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.maximum_factor { write!(s, "_mf{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.gamma { write!(s, "_g{}", float_to_var(v)).ok(); }
    if let Some(v) = ind.params.multiplier { write!(s, "_m{}", float_to_var(v)).ok(); }
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
        IndicatorType::Aroon => match field {
            "aroon_down" => 1,
            _ => 0, // "aroon_up" or default
        },
        IndicatorType::Fractal => match field {
            "fractal_down" => 1,
            _ => 0, // "fractal_up" or default
        },
        IndicatorType::HeikenAshi => match field {
            "ha_open" => 1,
            _ => 0, // "ha_close" or default
        },
        IndicatorType::Vortex => match field {
            "vi_minus" | "vortex_minus" => 1,
            _ => 0, // "vi_plus" or default
        },
        IndicatorType::Ichimoku => match field {
            "kijun" => 1,
            "senkou_a" => 2,
            "senkou_b" => 3,
            "chikou" => 4,
            _ => 0, // "tenkan" or default
        },
        IndicatorType::Fibonacci => match field {
            "level_382" => 1,
            "level_500" => 2,
            "level_618" => 3,
            "level_786" => 4,
            _ => 0, // "level_236" or default
        },
        IndicatorType::Pivots => match field {
            "r1" => 1,
            "s1" => 2,
            "r2" => 3,
            "s2" => 4,
            _ => 0, // "pp" or default
        },
        IndicatorType::KeltnerChannel => match field {
            "upper" => 1,
            "lower" => 2,
            _ => 0, // "middle" or default
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
        IndicatorType::BollingerBands | IndicatorType::KeltnerChannel => match field {
            "upper" => "_upper",
            "lower" => "_lower",
            _ => "_middle",
        },
        IndicatorType::Stochastic => match field {
            "D" | "d" => "_d",
            _ => "_k",
        },
        IndicatorType::Aroon => match field {
            "aroon_down" => "_down",
            _ => "_up",
        },
        IndicatorType::Fractal => match field {
            "fractal_down" => "_down",
            _ => "_up",
        },
        IndicatorType::HeikenAshi => match field {
            "ha_open" => "_open",
            _ => "_close",
        },
        IndicatorType::Vortex => match field {
            "vi_minus" => "_minus",
            _ => "_plus",
        },
        IndicatorType::Ichimoku => match field {
            "kijun" => "_kijun",
            "senkou_a" => "_senkou_a",
            "senkou_b" => "_senkou_b",
            "chikou" => "_chikou",
            _ => "_tenkan",
        },
        IndicatorType::Fibonacci => match field {
            "level_382" => "_382",
            "level_500" => "_500",
            "level_618" => "_618",
            "level_786" => "_786",
            _ => "_236",
        },
        IndicatorType::Pivots => match field {
            "r1" => "_r1",
            "r2" => "_r2",
            "r3" => "_r3",
            "s1" => "_s1",
            "s2" => "_s2",
            "s3" => "_s3",
            _ => "_pp",
        },
        IndicatorType::ADX => match field {
            "+DI" | "plus_di" => "_pdi",
            "-DI" | "minus_di" => "_mdi",
            _ => "",
        },
        _ => "",
    }
}

fn is_multi_output(ind_type: IndicatorType) -> bool {
    matches!(ind_type,
        IndicatorType::MACD | IndicatorType::BollingerBands | IndicatorType::Stochastic |
        IndicatorType::Aroon | IndicatorType::Fractal | IndicatorType::HeikenAshi |
        IndicatorType::Vortex | IndicatorType::KeltnerChannel | IndicatorType::Ichimoku |
        IndicatorType::Fibonacci | IndicatorType::Pivots
    )
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
        PositionSizingType::AntiMartingale =>
            writeln!(out, "input double InpRiskPct = {:.1};       // Risk % per Trade (AntiMartingale)", strategy.position_sizing.value).ok(),
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
            IndicatorType::VWAP | IndicatorType::AwesomeOscillator |
            IndicatorType::BarRange | IndicatorType::Fractal |
            IndicatorType::HeikenAshi | IndicatorType::TrueRange |
            IndicatorType::Pivots => {
                writeln!(out, "// NOTE: {:?} requires custom implementation in MQL5", ind.config.indicator_type).ok();
            }
            IndicatorType::Ichimoku => {
                writeln!(out, "input int    Inp_{}_tenkan = {};", ind.var_name, p.fast_period.unwrap_or(9)).ok();
                writeln!(out, "input int    Inp_{}_kijun = {};", ind.var_name, p.slow_period.unwrap_or(26)).ok();
                writeln!(out, "input int    Inp_{}_senkou = {};", ind.var_name, p.signal_period.unwrap_or(52)).ok();
            }
            IndicatorType::KeltnerChannel | IndicatorType::SuperTrend => {
                if let Some(period) = p.period {
                    writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, period).ok();
                }
                if let Some(mult) = p.multiplier {
                    writeln!(out, "input double Inp_{}_mult = {:.1};", ind.var_name, mult).ok();
                }
            }
            IndicatorType::LaguerreRSI => {
                if let Some(gamma) = p.gamma {
                    writeln!(out, "input double Inp_{}_gamma = {:.2};", ind.var_name, gamma).ok();
                }
            }
            _ => {
                // Period-only indicators (Aroon, BiggestRange, HighestInRange, etc.)
                if let Some(period) = p.period {
                    writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, period).ok();
                }
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
            IndicatorType::Ichimoku => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Ichimoku\", Inp_{0}_tenkan, Inp_{0}_kijun, Inp_{0}_senkou)",
                ind.var_name
            ),
            IndicatorType::KeltnerChannel => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_KeltnerChannel\", Inp_{0}_period, Inp_{0}_mult)",
                ind.var_name
            ),
            IndicatorType::SuperTrend => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_SuperTrend\", Inp_{0}_period, Inp_{0}_mult)",
                ind.var_name
            ),
            IndicatorType::LaguerreRSI => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LaguerreRSI\", Inp_{}_gamma)",
                ind.var_name
            ),
            // --- Custom BT_* indicators (all use app's own implementation) ---
            IndicatorType::BearsPower => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BearsPower\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::BullsPower => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BullsPower\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::DeMarker => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_DeMarker\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::AwesomeOscillator => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_AwesomeOscillator\")"
            ),
            IndicatorType::BarRange => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BarRange\")"
            ),
            IndicatorType::TrueRange => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_TrueRange\")"
            ),
            IndicatorType::Momentum => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Momentum\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::LinearRegression => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LinearRegression\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::Fractal => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Fractal\")"
            ),
            IndicatorType::StdDev => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_StdDev\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::HeikenAshi => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HeikenAshi\")"
            ),
            // --- Extended indicators that need generated custom .mq5 files ---
            _ => {
                let type_name = format!("{:?}", ind.config.indicator_type);
                if ind.config.params.period.is_some() {
                    format!(
                        "iCustom(_Symbol, PERIOD_CURRENT, \"BT_{}\", Inp_{}_period)",
                        type_name, ind.var_name
                    )
                } else {
                    format!(
                        "iCustom(_Symbol, PERIOD_CURRENT, \"BT_{}\")",
                        type_name
                    )
                }
            }
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

/// Returns true if a rule set has any rules, whether in flat list or groups.
fn has_rules(rules: &[Rule], groups: &[RuleGroup]) -> bool {
    !rules.is_empty() || groups.iter().any(|g| !g.rules.is_empty())
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

    let has_long_entry  = has_rules(&strategy.long_entry_rules,  &strategy.long_entry_groups);
    let has_short_entry = has_rules(&strategy.short_entry_rules, &strategy.short_entry_groups);
    let has_long_exit   = has_rules(&strategy.long_exit_rules,   &strategy.long_exit_groups);
    let has_short_exit  = has_rules(&strategy.short_exit_rules,  &strategy.short_exit_groups);

    if can_long && has_long_entry {
        writeln!(out, "      {}if(CheckLongEntry())", guard).ok();
        writeln!(out, "         OpenLong();").ok();
    } else if can_long {
        writeln!(out, "      // WARNING: No long entry rules defined").ok();
    }

    if can_short && has_short_entry {
        let kw = if can_long && has_long_entry { "else if" } else { "if" };
        writeln!(out, "      {}{}(CheckShortEntry())", if guard.is_empty() { "" } else { "else " }, kw).ok();
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
        if has_long_exit {
            writeln!(out, "      if(posType == POSITION_TYPE_BUY && CheckLongExit())").ok();
            writeln!(out, "         ClosePosition();").ok();
        }
    }
    if can_short {
        let kw = if can_long && has_long_exit { "else if" } else { "if" };
        if has_short_exit {
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

/// Flatten all rules from rule groups into a Vec<Rule> (for buffer collection).
fn all_rules_from_groups(groups: &[RuleGroup]) -> Vec<Rule> {
    groups.iter().flat_map(|g| g.rules.iter().cloned()).collect()
}

/// Emit one rule expression (shared between flat and group paths).
fn mql5_rule_expr(rule: &Rule, indicators: &[UniqueIndicator]) -> String {
    let left_curr = mql5_operand_expr(&rule.left_operand, 0, indicators);
    let right_curr = mql5_operand_expr(&rule.right_operand, 0, indicators);
    match rule.comparator {
        Comparator::GreaterThan   => format!("{} > {}",  left_curr, right_curr),
        Comparator::LessThan      => format!("{} < {}",  left_curr, right_curr),
        Comparator::GreaterOrEqual => format!("{} >= {}", left_curr, right_curr),
        Comparator::LessOrEqual   => format!("{} <= {}", left_curr, right_curr),
        Comparator::Equal         => format!("{} == {}", left_curr, right_curr),
        Comparator::CrossAbove => {
            let lp = mql5_operand_expr(&rule.left_operand,  1, indicators);
            let rp = mql5_operand_expr(&rule.right_operand, 1, indicators);
            format!("({} <= {} && {} > {})", lp, rp, left_curr, right_curr)
        }
        Comparator::CrossBelow => {
            let lp = mql5_operand_expr(&rule.left_operand,  1, indicators);
            let rp = mql5_operand_expr(&rule.right_operand, 1, indicators);
            format!("({} >= {} && {} < {})", lp, rp, left_curr, right_curr)
        }
    }
}

/// Emit the CopyBuffer declarations for a slice of rules.
fn mql5_emit_buffers(out: &mut String, rules: &[Rule], indicators: &[UniqueIndicator]) {
    let needed = collect_indicators_from_rules(rules);
    for ind_key in &needed {
        if let Some(ind) = indicators.iter().find(|i| i.config.cache_key() == *ind_key) {
            for buf_idx in collect_buffers_used(rules, ind) {
                let suffix = buffer_suffix(ind.config.indicator_type, buf_idx);
                writeln!(out, "   double {}{}[];", ind.var_name, suffix).ok();
                writeln!(out, "   ArraySetAsSeries({}{}, true);", ind.var_name, suffix).ok();
                writeln!(out, "   if(CopyBuffer({}, {}, 0, 3, {}{}) < 3) return false;",
                    ind.handle_name, buf_idx, ind.var_name, suffix).ok();
            }
        }
    }
}

fn mql5_check_rules_fn(out: &mut String, rules: &[Rule], groups: &[RuleGroup], fn_name: &str, indicators: &[UniqueIndicator]) {
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "bool {}()", fn_name).ok();
    writeln!(out, "{{").ok();

    // ── Groups path: when non-empty, groups take precedence over flat rules ──
    if !groups.is_empty() {
        let all_rules = all_rules_from_groups(groups);

        if all_rules.is_empty() {
            writeln!(out, "   // WARNING: No rules defined — always returns false").ok();
            writeln!(out, "   return false;").ok();
            writeln!(out, "}}").ok();
            writeln!(out).ok();
            return;
        }

        mql5_emit_buffers(out, &all_rules, indicators);
        writeln!(out).ok();

        // Evaluate each non-empty group
        let non_empty: Vec<&RuleGroup> = groups.iter().filter(|g| !g.rules.is_empty()).collect();
        for (gi, group) in non_empty.iter().enumerate() {
            let gn = gi + 1;
            for (ri, rule) in group.rules.iter().enumerate() {
                writeln!(out, "   bool g{}r{} = {};", gn, ri + 1, mql5_rule_expr(rule, indicators)).ok();
            }
            let int_op = if group.internal == LogicalOperator::Or { "||" } else { "&&" };
            let mut gexpr = format!("g{}r1", gn);
            for i in 1..group.rules.len() {
                write!(gexpr, " {} g{}r{}", int_op, gn, i + 1).ok();
            }
            writeln!(out, "   bool group{} = ({});", gn, gexpr).ok();
            writeln!(out).ok();
        }

        // Combine groups
        let mut combined = "group1".to_string();
        for i in 1..non_empty.len() {
            let join_op = match non_empty[i - 1].join {
                Some(LogicalOperator::Or) => "||",
                _ => "&&",
            };
            write!(combined, " {} group{}", join_op, i + 1).ok();
        }
        writeln!(out, "   return {};", combined).ok();
        writeln!(out, "}}").ok();
        writeln!(out).ok();
        return;
    }

    // ── Flat rules path ──
    if rules.is_empty() {
        writeln!(out, "   // WARNING: No rules defined — always returns false").ok();
        writeln!(out, "   return false;").ok();
        writeln!(out, "}}").ok();
        writeln!(out).ok();
        return;
    }

    mql5_emit_buffers(out, rules, indicators);
    writeln!(out).ok();

    for (i, rule) in rules.iter().enumerate() {
        writeln!(out, "   bool rule{} = {};", i + 1, mql5_rule_expr(rule, indicators)).ok();
    }
    writeln!(out).ok();

    let mut combined = "rule1".to_string();
    for (i, _rule) in rules.iter().enumerate().skip(1) {
        let op = if let Some(LogicalOperator::Or) = rules[i - 1].logical_operator { "||" } else { "&&" };
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
        IndicatorType::BollingerBands | IndicatorType::KeltnerChannel => match buf_idx { 1 => "_upper", 2 => "_lower", _ => "_middle" },
        IndicatorType::Stochastic => match buf_idx { 1 => "_d", _ => "_k" },
        IndicatorType::ADX => match buf_idx { 1 => "_pdi", 2 => "_mdi", _ => "_val" },
        IndicatorType::Aroon => match buf_idx { 1 => "_down", _ => "_up" },
        IndicatorType::Fractal => match buf_idx { 1 => "_down", _ => "_up" },
        IndicatorType::HeikenAshi => match buf_idx { 1 => "_open", _ => "_close" },
        IndicatorType::Vortex => match buf_idx { 1 => "_minus", _ => "_plus" },
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
                PriceField::DailyOpen => "iOpen(_Symbol, PERIOD_D1, ",
                PriceField::DailyHigh => "iHigh(_Symbol, PERIOD_D1, ",
                PriceField::DailyLow => "iLow(_Symbol, PERIOD_D1, ",
                PriceField::DailyClose => "iClose(_Symbol, PERIOD_D1, ",
            };
            match operand.price_field.unwrap_or(PriceField::Close) {
                PriceField::DailyOpen | PriceField::DailyHigh |
                PriceField::DailyLow | PriceField::DailyClose => {
                    format!("{}{})", func, offset)
                }
                _ => format!("{}(_Symbol, PERIOD_CURRENT, {})", func, offset),
            }
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
        OperandType::BarTime => {
            match operand.time_field {
                Some(TimeField::CurrentBar) => format!("(Bars(_Symbol, PERIOD_CURRENT) - 1 - {})", offset),
                Some(TimeField::BarTimeValue) | Some(TimeField::CurrentTime) =>
                    format!("(BT_Hour(iTime(_Symbol,PERIOD_CURRENT,{o})) * 60 + BT_Minute(iTime(_Symbol,PERIOD_CURRENT,{o})))", o=offset),
                Some(TimeField::BarHour) | Some(TimeField::CurrentHour) =>
                    format!("BT_Hour(iTime(_Symbol,PERIOD_CURRENT,{}))", offset),
                Some(TimeField::BarMinute) | Some(TimeField::CurrentMinute) =>
                    format!("BT_Minute(iTime(_Symbol,PERIOD_CURRENT,{}))", offset),
                Some(TimeField::BarDayOfWeek) | Some(TimeField::CurrentDayOfWeek) =>
                    format!("BT_DayOfWeek(iTime(_Symbol,PERIOD_CURRENT,{}))", offset),
                Some(TimeField::CurrentMonth) =>
                    format!("BT_Month(iTime(_Symbol,PERIOD_CURRENT,{}))", offset),
                None => "0 /* no time field */".into(),
            }
        }
        OperandType::CandlePattern => {
            let o = offset;
            match operand.candle_pattern {
                Some(CandlePatternType::Doji) =>
                    format!("(MathAbs(iClose(_Symbol,PERIOD_CURRENT,{o})-iOpen(_Symbol,PERIOD_CURRENT,{o})) <= 0.1*(iHigh(_Symbol,PERIOD_CURRENT,{o})-iLow(_Symbol,PERIOD_CURRENT,{o})) ? 1.0 : 0.0)"),
                Some(CandlePatternType::Hammer) =>
                    format!("(MathMin(iOpen(_Symbol,PERIOD_CURRENT,{o}),iClose(_Symbol,PERIOD_CURRENT,{o}))-iLow(_Symbol,PERIOD_CURRENT,{o}) >= 2.0*MathAbs(iClose(_Symbol,PERIOD_CURRENT,{o})-iOpen(_Symbol,PERIOD_CURRENT,{o})) && iHigh(_Symbol,PERIOD_CURRENT,{o})-MathMax(iOpen(_Symbol,PERIOD_CURRENT,{o}),iClose(_Symbol,PERIOD_CURRENT,{o})) <= MathAbs(iClose(_Symbol,PERIOD_CURRENT,{o})-iOpen(_Symbol,PERIOD_CURRENT,{o})) ? 1.0 : 0.0)"),
                Some(CandlePatternType::ShootingStar) =>
                    format!("(iHigh(_Symbol,PERIOD_CURRENT,{o})-MathMax(iOpen(_Symbol,PERIOD_CURRENT,{o}),iClose(_Symbol,PERIOD_CURRENT,{o})) >= 2.0*MathAbs(iClose(_Symbol,PERIOD_CURRENT,{o})-iOpen(_Symbol,PERIOD_CURRENT,{o})) && MathMin(iOpen(_Symbol,PERIOD_CURRENT,{o}),iClose(_Symbol,PERIOD_CURRENT,{o}))-iLow(_Symbol,PERIOD_CURRENT,{o}) <= MathAbs(iClose(_Symbol,PERIOD_CURRENT,{o})-iOpen(_Symbol,PERIOD_CURRENT,{o})) ? 1.0 : 0.0)"),
                Some(CandlePatternType::BullishEngulfing) =>
                    format!("(iClose(_Symbol,PERIOD_CURRENT,{o}+1)<iOpen(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})>iOpen(_Symbol,PERIOD_CURRENT,{o}) && iOpen(_Symbol,PERIOD_CURRENT,{o})<=iClose(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})>=iOpen(_Symbol,PERIOD_CURRENT,{o}+1) ? 1.0 : 0.0)"),
                Some(CandlePatternType::BearishEngulfing) =>
                    format!("(iClose(_Symbol,PERIOD_CURRENT,{o}+1)>iOpen(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})<iOpen(_Symbol,PERIOD_CURRENT,{o}) && iOpen(_Symbol,PERIOD_CURRENT,{o})>=iClose(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})<=iOpen(_Symbol,PERIOD_CURRENT,{o}+1) ? 1.0 : 0.0)"),
                Some(CandlePatternType::DarkCloud) =>
                    format!("(iClose(_Symbol,PERIOD_CURRENT,{o}+1)>iOpen(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})<iOpen(_Symbol,PERIOD_CURRENT,{o}) && iOpen(_Symbol,PERIOD_CURRENT,{o})>iHigh(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})<(iOpen(_Symbol,PERIOD_CURRENT,{o}+1)+iClose(_Symbol,PERIOD_CURRENT,{o}+1))/2.0 ? 1.0 : 0.0)"),
                Some(CandlePatternType::PiercingLine) =>
                    format!("(iClose(_Symbol,PERIOD_CURRENT,{o}+1)<iOpen(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})>iOpen(_Symbol,PERIOD_CURRENT,{o}) && iOpen(_Symbol,PERIOD_CURRENT,{o})<iLow(_Symbol,PERIOD_CURRENT,{o}+1) && iClose(_Symbol,PERIOD_CURRENT,{o})>(iOpen(_Symbol,PERIOD_CURRENT,{o}+1)+iClose(_Symbol,PERIOD_CURRENT,{o}+1))/2.0 ? 1.0 : 0.0)"),
                None => "0 /* no candle pattern */".into(),
            }
        }
        OperandType::Compound => {
            let left = operand.compound_left.as_deref()
                .map(|l| mql5_operand_expr(l, extra_shift, indicators))
                .unwrap_or_else(|| "0.0".to_string());
            let right = operand.compound_right.as_deref()
                .map(|r| mql5_operand_expr(r, extra_shift, indicators))
                .unwrap_or_else(|| "0.0".to_string());
            let op_str = match &operand.compound_op {
                Some(ArithmeticOp::Add) => "+",
                Some(ArithmeticOp::Sub) => "-",
                Some(ArithmeticOp::Mul) => "*",
                Some(ArithmeticOp::Div) => "/",
                None => "+",
            };
            format!("({} {} {})", left, op_str, right)
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
    // Shared normalization helper emitted into every branch
    writeln!(out, "   double minLot = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN);").ok();
    writeln!(out, "   double maxLot = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MAX);").ok();
    writeln!(out, "   double step   = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_STEP);").ok();
    writeln!(out, "   if(step <= 0) step = 0.01;").ok();
    writeln!(out).ok();

    match strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots => {
            writeln!(out, "   double lots = InpLotSize;").ok();
        }
        PositionSizingType::FixedAmount => {
            writeln!(out, "   // Fixed Amount: risk exactly $X per trade based on SL distance").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue <= 0 || tickSize <= 0) return minLot;").ok();
            writeln!(out, "   double slDistance    = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   if(slMoneyPerLot <= 0) return minLot;").ok();
            writeln!(out, "   double lots = InpFixedAmount / slMoneyPerLot;").ok();
        }
        PositionSizingType::PercentEquity => {
            writeln!(out, "   // Percent Equity: risk equity*X% per trade based on SL distance").ok();
            writeln!(out, "   double equity    = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue <= 0 || tickSize <= 0) return minLot;").ok();
            writeln!(out, "   double riskAmount    = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slDistance    = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   if(slMoneyPerLot <= 0) return minLot;").ok();
            writeln!(out, "   double lots = riskAmount / slMoneyPerLot;").ok();
        }
        PositionSizingType::RiskBased => {
            writeln!(out, "   // Risk-based: risk equity*X% per trade based on SL distance").ok();
            writeln!(out, "   double equity    = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue <= 0 || tickSize <= 0) return minLot;").ok();
            writeln!(out, "   double riskAmount    = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slDistance    = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   if(slMoneyPerLot <= 0) return minLot;").ok();
            writeln!(out, "   double lots = riskAmount / slMoneyPerLot;").ok();
        }
        PositionSizingType::AntiMartingale => {
            writeln!(out, "   // AntiMartingale: risk-based sizing (consecutive-loss decay managed externally)").ok();
            writeln!(out, "   double equity    = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickValue = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue <= 0 || tickSize <= 0) return minLot;").ok();
            writeln!(out, "   double riskAmount    = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slDistance    = MathAbs(price - sl);").ok();
            writeln!(out, "   double slMoneyPerLot = (slDistance / tickSize) * tickValue;").ok();
            writeln!(out, "   if(slMoneyPerLot <= 0) return minLot;").ok();
            writeln!(out, "   double lots = riskAmount / slMoneyPerLot;").ok();
        }
    }

    // Normalize: floor to SYMBOL_VOLUME_STEP, then clamp to [min, max]
    writeln!(out).ok();
    writeln!(out, "   lots = MathFloor(lots / step) * step;").ok();
    writeln!(out, "   lots = MathMax(minLot, MathMin(maxLot, lots));").ok();
    writeln!(out, "   return lots;").ok();
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
                writeln!(out, "   if(CopyBuffer(handle_{}, 0, 0, 1, atrBuf) < 1) return 0;", var).ok();
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
                writeln!(out, "   if(CopyBuffer(handle_{}, 0, 0, 1, atrBuf) < 1) return 0;", var).ok();
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
            writeln!(out, "   if(CopyBuffer(handle_{}, 0, 0, 1, atrBuf) < 1) return;", var).ok();
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

/// Emit BT_Hour / BT_Minute / BT_DayOfWeek / BT_Month helpers
/// when the strategy uses BarTime operands (MQL4's TimeHour etc. don't exist in MQL5).
fn mql5_time_helpers(out: &mut String, strategy: &Strategy) {
    let uses_bar_time = [
        &strategy.long_entry_rules,
        &strategy.short_entry_rules,
        &strategy.long_exit_rules,
        &strategy.short_exit_rules,
    ]
    .iter()
    .any(|rules| {
        rules.iter().any(|r| {
            r.left_operand.operand_type == OperandType::BarTime
                || r.right_operand.operand_type == OperandType::BarTime
        })
    });

    if !uses_bar_time {
        return;
    }

    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "// Time helpers (MQL5 equivalents of MQL4 TimeHour etc.)").ok();
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "int BT_Hour(datetime t) {{ MqlDateTime s; TimeToStruct(t,s); return s.hour; }}").ok();
    writeln!(out, "int BT_Minute(datetime t) {{ MqlDateTime s; TimeToStruct(t,s); return s.min; }}").ok();
    writeln!(out, "int BT_DayOfWeek(datetime t) {{ MqlDateTime s; TimeToStruct(t,s); return s.day_of_week; }}").ok();
    writeln!(out, "int BT_Month(datetime t) {{ MqlDateTime s; TimeToStruct(t,s); return s.mon; }}").ok();
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
            IndicatorType::VWAP | IndicatorType::AwesomeOscillator |
            IndicatorType::BarRange | IndicatorType::Fractal |
            IndicatorType::HeikenAshi | IndicatorType::TrueRange |
            IndicatorType::Pivots => {} // no params
            IndicatorType::Ichimoku => {
                writeln!(out, "i_{}_tenkan = input.int({}, \"Ichimoku Tenkan\")", ind.var_name, p.fast_period.unwrap_or(9)).ok();
                writeln!(out, "i_{}_kijun = input.int({}, \"Ichimoku Kijun\")", ind.var_name, p.slow_period.unwrap_or(26)).ok();
                writeln!(out, "i_{}_senkou = input.int({}, \"Ichimoku Senkou B\")", ind.var_name, p.signal_period.unwrap_or(52)).ok();
            }
            IndicatorType::KeltnerChannel | IndicatorType::SuperTrend => {
                if let Some(period) = p.period {
                    writeln!(out, "i_{}_period = input.int({}, \"{:?} Period\")", ind.var_name, period, ind.config.indicator_type).ok();
                }
                if let Some(mult) = p.multiplier {
                    writeln!(out, "i_{}_mult = input.float({:.1}, \"{:?} Mult\")", ind.var_name, mult, ind.config.indicator_type).ok();
                }
            }
            IndicatorType::LaguerreRSI => {
                if let Some(gamma) = p.gamma {
                    writeln!(out, "i_{}_gamma = input.float({:.2}, \"Laguerre Gamma\")", ind.var_name, gamma).ok();
                }
            }
            _ => {
                // Period-only indicators
                if let Some(period) = p.period {
                    writeln!(out, "i_{}_period = input.int({}, \"{:?} Period\")", ind.var_name, period, ind.config.indicator_type).ok();
                }
            }
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
            IndicatorType::Aroon => {
                writeln!(out, "{0}_up = 100.0 * (i_{0}_period - ta.highestbars(high, i_{0}_period)) / i_{0}_period", ind.var_name).ok();
                writeln!(out, "{0}_down = 100.0 * (i_{0}_period - ta.lowestbars(low, i_{0}_period)) / i_{0}_period", ind.var_name).ok();
            }
            IndicatorType::AwesomeOscillator => {
                writeln!(out, "{} = ta.sma(hl2, 5) - ta.sma(hl2, 34)", ind.var_name).ok();
            }
            IndicatorType::BarRange => {
                writeln!(out, "{} = high - low", ind.var_name).ok();
            }
            IndicatorType::BiggestRange => {
                writeln!(out, "{0} = ta.highest(high - low, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::HighestInRange => {
                writeln!(out, "{0} = ta.highest(high, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::LowestInRange => {
                writeln!(out, "{0} = ta.lowest(low, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::SmallestRange => {
                writeln!(out, "{0} = ta.lowest(high - low, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::BearsPower => {
                writeln!(out, "{0} = low - ta.ema(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::BullsPower => {
                writeln!(out, "{0} = high - ta.ema(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::DeMarker => {
                writeln!(out, "// DeMarker (custom calculation)").ok();
                writeln!(out, "{0}_demax = math.max(high - high[1], 0)", ind.var_name).ok();
                writeln!(out, "{0}_demin = math.max(low[1] - low, 0)", ind.var_name).ok();
                writeln!(out, "{0}_sma_max = ta.sma({0}_demax, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_sma_min = ta.sma({0}_demin, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0} = {0}_sma_max / ({0}_sma_max + {0}_sma_min)", ind.var_name).ok();
            }
            IndicatorType::Fibonacci => {
                writeln!(out, "// Fibonacci retracement levels over period").ok();
                writeln!(out, "{0}_hh = ta.highest(high, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_ll = ta.lowest(low, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_range = {0}_hh - {0}_ll", ind.var_name).ok();
                writeln!(out, "{0}_236 = {0}_hh - {0}_range * 0.236", ind.var_name).ok();
                writeln!(out, "{0}_382 = {0}_hh - {0}_range * 0.382", ind.var_name).ok();
                writeln!(out, "{0}_500 = {0}_hh - {0}_range * 0.500", ind.var_name).ok();
                writeln!(out, "{0}_618 = {0}_hh - {0}_range * 0.618", ind.var_name).ok();
                writeln!(out, "{0}_786 = {0}_hh - {0}_range * 0.786", ind.var_name).ok();
            }
            IndicatorType::Fractal => {
                writeln!(out, "// Fractals (Williams 5-bar)").ok();
                writeln!(out, "{0}_up = (high[2] > high[4] and high[2] > high[3] and high[2] > high[1] and high[2] > high[0]) ? high[2] : na", ind.var_name).ok();
                writeln!(out, "{0}_down = (low[2] < low[4] and low[2] < low[3] and low[2] < low[1] and low[2] < low[0]) ? low[2] : na", ind.var_name).ok();
            }
            IndicatorType::GannHiLo => {
                writeln!(out, "// Gann HiLo Activator").ok();
                writeln!(out, "{0}_sma_h = ta.sma(high, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_sma_l = ta.sma(low, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0} = close > {0}_sma_h[1] ? {0}_sma_l : {0}_sma_h", ind.var_name).ok();
            }
            IndicatorType::HeikenAshi => {
                writeln!(out, "// Heiken Ashi").ok();
                writeln!(out, "{0}_close = (open + high + low + close) / 4", ind.var_name).ok();
                writeln!(out, "var float {0}_open = na", ind.var_name).ok();
                writeln!(out, "{0}_open := na({0}_open[1]) ? open : ({0}_open[1] + {0}_close[1]) / 2", ind.var_name).ok();
            }
            IndicatorType::HullMA => {
                writeln!(out, "{0} = ta.hma(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::Ichimoku => {
                writeln!(out, "// Ichimoku").ok();
                writeln!(out, "{0}_tenkan = (ta.highest(high, i_{0}_tenkan) + ta.lowest(low, i_{0}_tenkan)) / 2", ind.var_name).ok();
                writeln!(out, "{0}_kijun = (ta.highest(high, i_{0}_kijun) + ta.lowest(low, i_{0}_kijun)) / 2", ind.var_name).ok();
                writeln!(out, "{0}_senkou_a = ({0}_tenkan + {0}_kijun) / 2", ind.var_name).ok();
                writeln!(out, "{0}_senkou_b = (ta.highest(high, i_{0}_senkou) + ta.lowest(low, i_{0}_senkou)) / 2", ind.var_name).ok();
                writeln!(out, "{0}_chikou = close", ind.var_name).ok();
            }
            IndicatorType::KeltnerChannel => {
                writeln!(out, "// Keltner Channel").ok();
                writeln!(out, "{0}_middle = ta.ema(close, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_atr = ta.atr(i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_upper = {0}_middle + i_{0}_mult * {0}_atr", ind.var_name).ok();
                writeln!(out, "{0}_lower = {0}_middle - i_{0}_mult * {0}_atr", ind.var_name).ok();
            }
            IndicatorType::LaguerreRSI => {
                writeln!(out, "// Laguerre RSI (custom calculation)").ok();
                writeln!(out, "var float {0}_l0 = 0.0, var float {0}_l1 = 0.0, var float {0}_l2 = 0.0, var float {0}_l3 = 0.0", ind.var_name).ok();
                writeln!(out, "{0}_l0 := (1.0 - i_{0}_gamma) * close + i_{0}_gamma * nz({0}_l0[1])", ind.var_name).ok();
                writeln!(out, "{0}_l1 := -i_{0}_gamma * {0}_l0 + nz({0}_l0[1]) + i_{0}_gamma * nz({0}_l1[1])", ind.var_name).ok();
                writeln!(out, "{0}_l2 := -i_{0}_gamma * {0}_l1 + nz({0}_l1[1]) + i_{0}_gamma * nz({0}_l2[1])", ind.var_name).ok();
                writeln!(out, "{0}_l3 := -i_{0}_gamma * {0}_l2 + nz({0}_l2[1]) + i_{0}_gamma * nz({0}_l3[1])", ind.var_name).ok();
                writeln!(out, "{0}_cu = ({0}_l0 > {0}_l1 ? {0}_l0 - {0}_l1 : 0) + ({0}_l1 > {0}_l2 ? {0}_l1 - {0}_l2 : 0) + ({0}_l2 > {0}_l3 ? {0}_l2 - {0}_l3 : 0)", ind.var_name).ok();
                writeln!(out, "{0}_cd = ({0}_l1 > {0}_l0 ? {0}_l1 - {0}_l0 : 0) + ({0}_l2 > {0}_l1 ? {0}_l2 - {0}_l1 : 0) + ({0}_l3 > {0}_l2 ? {0}_l3 - {0}_l2 : 0)", ind.var_name).ok();
                writeln!(out, "{0} = {0}_cu + {0}_cd != 0 ? {0}_cu / ({0}_cu + {0}_cd) : 0", ind.var_name).ok();
            }
            IndicatorType::LinearRegression => {
                writeln!(out, "{0} = ta.linreg(close, i_{0}_period, 0)", ind.var_name).ok();
            }
            IndicatorType::Momentum => {
                writeln!(out, "{0} = ta.mom(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::SuperTrend => {
                writeln!(out, "[{0}, {0}_dir] = ta.supertrend(i_{0}_mult, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::TrueRange => {
                writeln!(out, "{} = ta.tr(true)", ind.var_name).ok();
            }
            IndicatorType::StdDev => {
                writeln!(out, "{0} = ta.stdev(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::Reflex => {
                writeln!(out, "// Reflex (Ehlers) — approximated with custom calc").ok();
                writeln!(out, "{0} = 2 * ta.sma(close, i_{0}_period) - ta.sma(ta.sma(close, i_{0}_period), i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::Pivots => {
                writeln!(out, "// Classic Pivots from prior daily bar").ok();
                writeln!(out, "[{0}_pp, {0}_r1, {0}_s1, {0}_r2, {0}_s2, {0}_r3, {0}_s3] = request.security(syminfo.tickerid, \"D\", [(high[1]+low[1]+close[1])/3, 2*(high[1]+low[1]+close[1])/3 - low[1], 2*(high[1]+low[1]+close[1])/3 - high[1], (high[1]+low[1]+close[1])/3 + high[1] - low[1], (high[1]+low[1]+close[1])/3 - high[1] + low[1], 2*((high[1]+low[1]+close[1])/3 - low[1]) + (high[1]+low[1]+close[1])/3, 2*((high[1]+low[1]+close[1])/3 - high[1]) + (high[1]+low[1]+close[1])/3])", ind.var_name).ok();
            }
            IndicatorType::UlcerIndex => {
                writeln!(out, "// Ulcer Index").ok();
                writeln!(out, "{0}_hh = ta.highest(close, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_pct_dd = 100 * (close - {0}_hh) / {0}_hh", ind.var_name).ok();
                writeln!(out, "{0} = math.sqrt(ta.sma({0}_pct_dd * {0}_pct_dd, i_{0}_period))", ind.var_name).ok();
            }
            IndicatorType::Vortex => {
                writeln!(out, "// Vortex Indicator").ok();
                writeln!(out, "{0}_vm_plus = math.abs(high - low[1])", ind.var_name).ok();
                writeln!(out, "{0}_vm_minus = math.abs(low - high[1])", ind.var_name).ok();
                writeln!(out, "{0}_tr = ta.tr(true)", ind.var_name).ok();
                writeln!(out, "{0}_plus = ta.sum({0}_vm_plus, i_{0}_period) / ta.sum({0}_tr, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_minus = ta.sum({0}_vm_minus, i_{0}_period) / ta.sum({0}_tr, i_{0}_period)", ind.var_name).ok();
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
                PriceField::DailyOpen => "request.security(syminfo.tickerid, \"D\", open)",
                PriceField::DailyHigh => "request.security(syminfo.tickerid, \"D\", high)",
                PriceField::DailyLow => "request.security(syminfo.tickerid, \"D\", low)",
                PriceField::DailyClose => "request.security(syminfo.tickerid, \"D\", close[1])",
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
        OperandType::BarTime => {
            match operand.time_field {
                Some(TimeField::CurrentBar) => format!("bar_index{}", offset_str),
                Some(TimeField::BarTimeValue) | Some(TimeField::CurrentTime) =>
                    format!("(hour{os} * 60 + minute{os})", os=offset_str),
                Some(TimeField::BarHour) | Some(TimeField::CurrentHour) =>
                    format!("hour{}", offset_str),
                Some(TimeField::BarMinute) | Some(TimeField::CurrentMinute) =>
                    format!("minute{}", offset_str),
                Some(TimeField::BarDayOfWeek) | Some(TimeField::CurrentDayOfWeek) =>
                    format!("dayofweek{}", offset_str),
                Some(TimeField::CurrentMonth) =>
                    format!("month{}", offset_str),
                None => "na".into(),
            }
        }
        OperandType::CandlePattern => {
            let os = &offset_str;
            match operand.candle_pattern {
                Some(CandlePatternType::Doji) =>
                    format!("(math.abs(close{os}-open{os}) <= 0.1*(high{os}-low{os}) ? 1.0 : 0.0)"),
                Some(CandlePatternType::Hammer) =>
                    format!("(math.min(open{os},close{os})-low{os} >= 2.0*math.abs(close{os}-open{os}) and high{os}-math.max(open{os},close{os}) <= math.abs(close{os}-open{os}) ? 1.0 : 0.0)"),
                Some(CandlePatternType::ShootingStar) =>
                    format!("(high{os}-math.max(open{os},close{os}) >= 2.0*math.abs(close{os}-open{os}) and math.min(open{os},close{os})-low{os} <= math.abs(close{os}-open{os}) ? 1.0 : 0.0)"),
                Some(CandlePatternType::BullishEngulfing) => {
                    let p = if offset_str.is_empty() { "[1]".to_string() } else { format!("[{}]", operand.offset.unwrap_or(0) + 1) };
                    format!("(close{p}<open{p} and close{os}>open{os} and open{os}<=close{p} and close{os}>=open{p} ? 1.0 : 0.0)")
                }
                Some(CandlePatternType::BearishEngulfing) => {
                    let p = if offset_str.is_empty() { "[1]".to_string() } else { format!("[{}]", operand.offset.unwrap_or(0) + 1) };
                    format!("(close{p}>open{p} and close{os}<open{os} and open{os}>=close{p} and close{os}<=open{p} ? 1.0 : 0.0)")
                }
                Some(CandlePatternType::DarkCloud) => {
                    let p = if offset_str.is_empty() { "[1]".to_string() } else { format!("[{}]", operand.offset.unwrap_or(0) + 1) };
                    format!("(close{p}>open{p} and close{os}<open{os} and open{os}>high{p} and close{os}<(open{p}+close{p})/2.0 ? 1.0 : 0.0)")
                }
                Some(CandlePatternType::PiercingLine) => {
                    let p = if offset_str.is_empty() { "[1]".to_string() } else { format!("[{}]", operand.offset.unwrap_or(0) + 1) };
                    format!("(close{p}<open{p} and close{os}>open{os} and open{os}<low{p} and close{os}>(open{p}+close{p})/2.0 ? 1.0 : 0.0)")
                }
                None => "na".into(),
            }
        }
        OperandType::Compound => {
            let left = operand.compound_left.as_deref()
                .map(|l| pine_operand_expr(l, extra_offset))
                .unwrap_or_else(|| "0.0".to_string());
            let right = operand.compound_right.as_deref()
                .map(|r| pine_operand_expr(r, extra_offset))
                .unwrap_or_else(|| "0.0".to_string());
            let op_str = match &operand.compound_op {
                Some(ArithmeticOp::Add) => "+",
                Some(ArithmeticOp::Sub) => "-",
                Some(ArithmeticOp::Mul) => "*",
                Some(ArithmeticOp::Div) => "/",
                None => "+",
            };
            format!("({} {} {})", left, op_str, right)
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
        // Extended indicators — custom MQL5 indicator files
        IndicatorType::Aroon => ("BT_Aroon.mq5".into(), gen_mql5_aroon()),
        IndicatorType::BarRange => ("BT_BarRange.mq5".into(), gen_mql5_bar_range()),
        IndicatorType::BiggestRange => ("BT_BiggestRange.mq5".into(), gen_mql5_biggest_range()),
        IndicatorType::SmallestRange => ("BT_SmallestRange.mq5".into(), gen_mql5_smallest_range()),
        IndicatorType::HighestInRange => ("BT_HighestInRange.mq5".into(), gen_mql5_highest_in_range()),
        IndicatorType::LowestInRange => ("BT_LowestInRange.mq5".into(), gen_mql5_lowest_in_range()),
        IndicatorType::GannHiLo => ("BT_GannHiLo.mq5".into(), gen_mql5_gann_hilo()),
        IndicatorType::HullMA => ("BT_HullMA.mq5".into(), gen_mql5_hull_ma()),
        IndicatorType::Vortex => ("BT_Vortex.mq5".into(), gen_mql5_vortex()),
        IndicatorType::TrueRange => ("BT_TrueRange.mq5".into(), gen_mql5_true_range()),
        IndicatorType::Fibonacci => ("BT_Fibonacci.mq5".into(), gen_mql5_fibonacci()),
        IndicatorType::KeltnerChannel => ("BT_KeltnerChannel.mq5".into(), gen_mql5_keltner_channel()),
        IndicatorType::SuperTrend => ("BT_SuperTrend.mq5".into(), gen_mql5_super_trend()),
        IndicatorType::LaguerreRSI => ("BT_LaguerreRSI.mq5".into(), gen_mql5_laguerre_rsi()),
        IndicatorType::Pivots => ("BT_Pivots.mq5".into(), gen_mql5_pivots()),
        // Native handles or already handled above — no file needed
        _ => return None,
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
#property indicator_buffers 3
#property indicator_plots   3
#property indicator_label1  "ADX"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  1
#property indicator_label2  "+DI"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrLime
#property indicator_width2  1
#property indicator_label3  "-DI"
#property indicator_type3   DRAW_LINE
#property indicator_color3  clrRed
#property indicator_width3  1
#property indicator_level1  25

input int InpPeriod = 14; // Period

double AdxBuffer[];
double PdiBuffer[];
double MdiBuffer[];

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
   SetIndexBuffer(1, PdiBuffer, INDICATOR_DATA);
   SetIndexBuffer(2, MdiBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod * 2);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetInteger(2, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   PlotIndexSetDouble(1, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   PlotIndexSetDouble(2, PLOT_EMPTY_VALUE, EMPTY_VALUE);
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
      ArrayInitialize(PdiBuffer, EMPTY_VALUE);
      ArrayInitialize(MdiBuffer, EMPTY_VALUE);

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
         PdiBuffer[InpPeriod] = pdi;
         MdiBuffer[InpPeriod] = mdi;
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
         PdiBuffer[i] = pdi;
         MdiBuffer[i] = mdi;
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
         PdiBuffer[i] = pdi;
         MdiBuffer[i] = mdi;
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
         PdiBuffer[i] = pdi;
         MdiBuffer[i] = mdi;
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
         PdiBuffer[i] = pdi;
         MdiBuffer[i] = mdi;
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

// ── BT_Aroon ──

fn gen_mql5_aroon() -> String {
    let mut out = mql5_indicator_header("BT_Aroon");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   2
#property indicator_label1  "Aroon Up"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_label2  "Aroon Down"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrCrimson

input int InpPeriod = 14;

double AroonUpBuffer[];
double AroonDownBuffer[];

int OnInit()
{
   SetIndexBuffer(0, AroonUpBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, AroonDownBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_Aroon(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod;
   for(int i = start; i < rates_total; i++)
   {
      int hi_pos = i - InpPeriod, lo_pos = i - InpPeriod;
      double hi_max = high[hi_pos], lo_min = low[lo_pos];
      for(int k = i - InpPeriod + 1; k <= i; k++)
      {
         if(high[k] >= hi_max) { hi_max = high[k]; hi_pos = k; }
         if(low[k]  <= lo_min) { lo_min = low[k];  lo_pos = k; }
      }
      AroonUpBuffer[i]   = 100.0 * (hi_pos - (i - InpPeriod)) / InpPeriod;
      AroonDownBuffer[i] = 100.0 * (lo_pos - (i - InpPeriod)) / InpPeriod;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_BarRange ──

fn gen_mql5_bar_range() -> String {
    let mut out = mql5_indicator_header("BT_BarRange");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Bar Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue

double BarRangeBuffer[];

int OnInit()
{
   SetIndexBuffer(0, BarRangeBuffer, INDICATOR_DATA);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_BarRange");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > 0) ? prev_calculated - 1 : 0;
   for(int i = start; i < rates_total; i++)
      BarRangeBuffer[i] = high[i] - low[i];
   return rates_total;
}
"#);
    out
}

// ── BT_BiggestRange ──

fn gen_mql5_biggest_range() -> String {
    let mut out = mql5_indicator_header("BT_BiggestRange");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Biggest Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrOrange

input int InpPeriod = 14;

double BiggestRangeBuffer[];

int OnInit()
{
   SetIndexBuffer(0, BiggestRangeBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_BiggestRange(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod - 1;
   for(int i = start; i < rates_total; i++)
   {
      double biggest = 0;
      for(int k = 0; k < InpPeriod && (i - k) >= 0; k++)
      {
         double r = high[i - k] - low[i - k];
         if(r > biggest) biggest = r;
      }
      BiggestRangeBuffer[i] = biggest;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_SmallestRange ──

fn gen_mql5_smallest_range() -> String {
    let mut out = mql5_indicator_header("BT_SmallestRange");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Smallest Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrMediumVioletRed

input int InpPeriod = 14;

double SmallestRangeBuffer[];

int OnInit()
{
   SetIndexBuffer(0, SmallestRangeBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_SmallestRange(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod - 1;
   for(int i = start; i < rates_total; i++)
   {
      double smallest = DBL_MAX;
      for(int k = 0; k < InpPeriod && (i - k) >= 0; k++)
      {
         double r = high[i - k] - low[i - k];
         if(r < smallest) smallest = r;
      }
      SmallestRangeBuffer[i] = (smallest == DBL_MAX) ? 0 : smallest;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_HighestInRange ──

fn gen_mql5_highest_in_range() -> String {
    let mut out = mql5_indicator_header("BT_HighestInRange");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Highest In Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrLimeGreen

input int InpPeriod = 14;

double HighestBuffer[];

int OnInit()
{
   SetIndexBuffer(0, HighestBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_HighestInRange(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod - 1;
   for(int i = start; i < rates_total; i++)
   {
      double highest = -DBL_MAX;
      for(int k = 0; k < InpPeriod && (i - k) >= 0; k++)
         if(high[i - k] > highest) highest = high[i - k];
      HighestBuffer[i] = (highest == -DBL_MAX) ? 0 : highest;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_LowestInRange ──

fn gen_mql5_lowest_in_range() -> String {
    let mut out = mql5_indicator_header("BT_LowestInRange");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Lowest In Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrCrimson

input int InpPeriod = 14;

double LowestBuffer[];

int OnInit()
{
   SetIndexBuffer(0, LowestBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_LowestInRange(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod - 1;
   for(int i = start; i < rates_total; i++)
   {
      double lowest = DBL_MAX;
      for(int k = 0; k < InpPeriod && (i - k) >= 0; k++)
         if(low[i - k] < lowest) lowest = low[i - k];
      LowestBuffer[i] = (lowest == DBL_MAX) ? 0 : lowest;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_GannHiLo ──

fn gen_mql5_gann_hilo() -> String {
    let mut out = mql5_indicator_header("BT_GannHiLo");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Gann HiLo"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_width1  2

input int InpPeriod = 3;

double GannBuffer[];

int OnInit()
{
   SetIndexBuffer(0, GannBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_GannHiLo(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod;
   for(int i = start; i < rates_total; i++)
   {
      double sma_h = 0, sma_l = 0;
      for(int k = 0; k < InpPeriod; k++) { sma_h += high[i - k]; sma_l += low[i - k]; }
      sma_h /= InpPeriod;
      sma_l /= InpPeriod;
      // prev bar sma_h
      double prev_sma_h = 0;
      for(int k = 0; k < InpPeriod; k++) prev_sma_h += high[i - 1 - k];
      prev_sma_h /= InpPeriod;
      GannBuffer[i] = (close[i - 1] > prev_sma_h) ? sma_l : sma_h;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_HullMA ──

fn gen_mql5_hull_ma() -> String {
    let mut out = mql5_indicator_header("BT_HullMA");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Hull MA"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrMediumPurple
#property indicator_width1  2

input int InpPeriod = 16;

double HullBuffer[];

// Weighted Moving Average helper
double WMA(const double &arr[], int idx, int period)
{
   if(idx < period - 1) return EMPTY_VALUE;
   double sum = 0, wsum = 0;
   for(int k = 0; k < period; k++)
   {
      double w = period - k;
      sum  += arr[idx - k] * w;
      wsum += w;
   }
   return (wsum > 0) ? sum / wsum : 0;
}

int OnInit()
{
   SetIndexBuffer(0, HullBuffer, INDICATOR_DATA);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   int sqrtPeriod = (int)MathSqrt(InpPeriod);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod + sqrtPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_HullMA(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int half = InpPeriod / 2;
   int sqrtn = (int)MathSqrt(InpPeriod);
   // Need a temp buffer for the intermediate WMA series
   double tmp[];
   ArrayResize(tmp, rates_total);

   int start = (prev_calculated > 0) ? prev_calculated - 1 : 0;
   for(int i = start; i < rates_total; i++)
   {
      double wma_half = WMA(close, i, half);
      double wma_full = WMA(close, i, InpPeriod);
      if(wma_half == EMPTY_VALUE || wma_full == EMPTY_VALUE)
         tmp[i] = EMPTY_VALUE;
      else
         tmp[i] = 2.0 * wma_half - wma_full;
   }
   for(int i = start; i < rates_total; i++)
   {
      double h = WMA(tmp, i, sqrtn);
      HullBuffer[i] = (h == EMPTY_VALUE) ? EMPTY_VALUE : h;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_Vortex ──

fn gen_mql5_vortex() -> String {
    let mut out = mql5_indicator_header("BT_Vortex");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   2
#property indicator_label1  "VI+"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_label2  "VI-"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrCrimson

input int InpPeriod = 14;

double VIplusBuffer[];
double VIminusBuffer[];

int OnInit()
{
   SetIndexBuffer(0, VIplusBuffer,  INDICATOR_DATA);
   SetIndexBuffer(1, VIminusBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_Vortex(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod;
   for(int i = start; i < rates_total; i++)
   {
      double vm_plus = 0, vm_minus = 0, tr_sum = 0;
      for(int k = 0; k < InpPeriod; k++)
      {
         int cur = i - k, prv = i - k - 1;
         vm_plus  += MathAbs(high[cur] - low[prv]);
         vm_minus += MathAbs(low[cur]  - high[prv]);
         double tr = MathMax(high[cur] - low[cur],
                    MathMax(MathAbs(high[cur] - close[prv]),
                            MathAbs(low[cur]  - close[prv])));
         tr_sum += tr;
      }
      VIplusBuffer[i]  = (tr_sum > 0) ? vm_plus  / tr_sum : 0;
      VIminusBuffer[i] = (tr_sum > 0) ? vm_minus / tr_sum : 0;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_TrueRange ──

fn gen_mql5_true_range() -> String {
    let mut out = mql5_indicator_header("BT_TrueRange");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "True Range"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrOrange

double TRBuffer[];

int OnInit()
{
   SetIndexBuffer(0, TRBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_TrueRange");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > 1) ? prev_calculated - 1 : 1;
   for(int i = start; i < rates_total; i++)
   {
      double hl = high[i] - low[i];
      double hc = MathAbs(high[i] - close[i - 1]);
      double lc = MathAbs(low[i]  - close[i - 1]);
      TRBuffer[i] = MathMax(hl, MathMax(hc, lc));
   }
   return rates_total;
}
"#);
    out
}

// ── BT_Fibonacci ──

fn gen_mql5_fibonacci() -> String {
    let mut out = mql5_indicator_header("BT_Fibonacci");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 5
#property indicator_plots   5
#property indicator_label1  "Fib 23.6%"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_label2  "Fib 38.2%"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrYellow
#property indicator_label3  "Fib 50%"
#property indicator_type3   DRAW_LINE
#property indicator_color3  clrOrange
#property indicator_label4  "Fib 61.8%"
#property indicator_type4   DRAW_LINE
#property indicator_color4  clrDarkOrange
#property indicator_label5  "Fib 78.6%"
#property indicator_type5   DRAW_LINE
#property indicator_color5  clrOrangeRed

input int InpPeriod = 20;

double Fib236[], Fib382[], Fib500[], Fib618[], Fib786[];

int OnInit()
{
   SetIndexBuffer(0, Fib236, INDICATOR_DATA);
   SetIndexBuffer(1, Fib382, INDICATOR_DATA);
   SetIndexBuffer(2, Fib500, INDICATOR_DATA);
   SetIndexBuffer(3, Fib618, INDICATOR_DATA);
   SetIndexBuffer(4, Fib786, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(2, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(3, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(4, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_Fibonacci(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod - 1;
   for(int i = start; i < rates_total; i++)
   {
      double hh = -DBL_MAX, ll = DBL_MAX;
      for(int k = 0; k < InpPeriod && (i - k) >= 0; k++)
      {
         if(high[i - k] > hh) hh = high[i - k];
         if(low[i - k]  < ll) ll = low[i - k];
      }
      double rng = hh - ll;
      Fib236[i] = hh - rng * 0.236;
      Fib382[i] = hh - rng * 0.382;
      Fib500[i] = hh - rng * 0.500;
      Fib618[i] = hh - rng * 0.618;
      Fib786[i] = hh - rng * 0.786;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_KeltnerChannel ──

fn gen_mql5_keltner_channel() -> String {
    let mut out = mql5_indicator_header("BT_KeltnerChannel");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 3
#property indicator_plots   3
#property indicator_label1  "KC Middle"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_width1  2
#property indicator_label2  "KC Upper"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrSteelBlue
#property indicator_label3  "KC Lower"
#property indicator_type3   DRAW_LINE
#property indicator_color3  clrSteelBlue

input int    InpPeriod = 20;
input double InpMult   = 1.5;

double MiddleBuffer[], UpperBuffer[], LowerBuffer[];

int OnInit()
{
   SetIndexBuffer(0, MiddleBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, UpperBuffer,  INDICATOR_DATA);
   SetIndexBuffer(2, LowerBuffer,  INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, InpPeriod - 1);
   PlotIndexSetInteger(2, PLOT_DRAW_BEGIN, InpPeriod - 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_KeltnerChannel(" + IntegerToString(InpPeriod) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   if(rates_total < InpPeriod + 1) return 0;
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod;
   for(int i = start; i < rates_total; i++)
   {
      // EMA-style middle (SMA for simplicity to match Rust impl)
      double ema_sum = 0;
      for(int k = 0; k < InpPeriod; k++) ema_sum += close[i - k];
      double mid = ema_sum / InpPeriod;
      // ATR
      double atr_sum = 0;
      for(int k = 0; k < InpPeriod; k++)
      {
         int cur = i - k, prv = i - k - 1;
         if(prv < 0) { atr_sum += high[cur] - low[cur]; continue; }
         double tr = MathMax(high[cur] - low[cur],
                    MathMax(MathAbs(high[cur] - close[prv]),
                            MathAbs(low[cur]  - close[prv])));
         atr_sum += tr;
      }
      double atr = atr_sum / InpPeriod;
      MiddleBuffer[i] = mid;
      UpperBuffer[i]  = mid + InpMult * atr;
      LowerBuffer[i]  = mid - InpMult * atr;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_SuperTrend ──

fn gen_mql5_super_trend() -> String {
    let mut out = mql5_indicator_header("BT_SuperTrend");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "SuperTrend"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_width1  2

input int    InpPeriod = 10;
input double InpMult   = 3.0;

double STBuffer[];

int OnInit()
{
   SetIndexBuffer(0, STBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, InpPeriod);
   PlotIndexSetDouble(0, PLOT_EMPTY_VALUE, EMPTY_VALUE);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_SuperTrend(" + IntegerToString(InpPeriod) + "," + DoubleToString(InpMult, 1) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   if(rates_total < InpPeriod + 1) return 0;
   // Wilder ATR + SuperTrend
   double atr[];
   ArrayResize(atr, rates_total);
   atr[0] = high[0] - low[0];
   for(int i = 1; i < rates_total; i++)
   {
      double tr = MathMax(high[i] - low[i],
                 MathMax(MathAbs(high[i] - close[i-1]),
                         MathAbs(low[i]  - close[i-1])));
      atr[i] = (atr[i-1] * (InpPeriod - 1) + tr) / InpPeriod;
   }
   double upper = 0, lower = 0;
   bool trend_up = true;
   int start = (prev_calculated > InpPeriod) ? prev_calculated - 1 : InpPeriod;
   for(int i = InpPeriod; i < rates_total; i++)
   {
      double hl2 = (high[i] + low[i]) / 2.0;
      double basic_upper = hl2 + InpMult * atr[i];
      double basic_lower = hl2 - InpMult * atr[i];
      if(i == InpPeriod) { upper = basic_upper; lower = basic_lower; trend_up = close[i] >= lower; }
      else
      {
         upper = (basic_upper < upper || close[i-1] > upper) ? basic_upper : upper;
         lower = (basic_lower > lower || close[i-1] < lower) ? basic_lower : lower;
         if(trend_up  && close[i] < lower) trend_up = false;
         if(!trend_up && close[i] > upper) trend_up = true;
      }
      STBuffer[i] = trend_up ? lower : upper;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_LaguerreRSI ──

fn gen_mql5_laguerre_rsi() -> String {
    let mut out = mql5_indicator_header("BT_LaguerreRSI");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "Laguerre RSI"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
#property indicator_levels 2
#property indicator_levelvalue1 0.8
#property indicator_levelvalue2 0.2

input double InpGamma = 0.7;

double LRSIBuffer[];
double L0prev = 0, L1prev = 0, L2prev = 0, L3prev = 0;

int OnInit()
{
   SetIndexBuffer(0, LRSIBuffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, 4);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_LaguerreRSI(" + DoubleToString(InpGamma, 2) + ")");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   int start = (prev_calculated > 1) ? prev_calculated - 1 : 0;
   if(prev_calculated == 0) { L0prev = L1prev = L2prev = L3prev = 0; }
   for(int i = start; i < rates_total; i++)
   {
      double L0 = (1 - InpGamma) * close[i] + InpGamma * L0prev;
      double L1 = -InpGamma * L0 + L0prev + InpGamma * L1prev;
      double L2 = -InpGamma * L1 + L1prev + InpGamma * L2prev;
      double L3 = -InpGamma * L2 + L2prev + InpGamma * L3prev;
      double cu = 0, cd = 0;
      if(L0 >= L1) cu += L0 - L1; else cd += L1 - L0;
      if(L1 >= L2) cu += L1 - L2; else cd += L2 - L1;
      if(L2 >= L3) cu += L2 - L3; else cd += L3 - L2;
      LRSIBuffer[i] = (cu + cd > 0) ? cu / (cu + cd) : 0;
      L0prev = L0; L1prev = L1; L2prev = L2; L3prev = L3;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_Pivots ──

fn gen_mql5_pivots() -> String {
    let mut out = mql5_indicator_header("BT_Pivots");
    out.push_str(r#"#property indicator_chart_window
#property indicator_buffers 5
#property indicator_plots   5
#property indicator_label1  "PP"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGold
#property indicator_width1  2
#property indicator_label2  "R1"
#property indicator_type2   DRAW_LINE
#property indicator_color2  clrLimeGreen
#property indicator_label3  "S1"
#property indicator_type3   DRAW_LINE
#property indicator_color3  clrCrimson
#property indicator_label4  "R2"
#property indicator_type4   DRAW_LINE
#property indicator_color4  clrGreen
#property indicator_label5  "S2"
#property indicator_type5   DRAW_LINE
#property indicator_color5  clrRed

double PPBuffer[], R1Buffer[], S1Buffer[], R2Buffer[], S2Buffer[];

int OnInit()
{
   SetIndexBuffer(0, PPBuffer, INDICATOR_DATA);
   SetIndexBuffer(1, R1Buffer, INDICATOR_DATA);
   SetIndexBuffer(2, S1Buffer, INDICATOR_DATA);
   SetIndexBuffer(3, R2Buffer, INDICATOR_DATA);
   SetIndexBuffer(4, S2Buffer, INDICATOR_DATA);
   PlotIndexSetInteger(0, PLOT_DRAW_BEGIN, 1);
   PlotIndexSetInteger(1, PLOT_DRAW_BEGIN, 1);
   PlotIndexSetInteger(2, PLOT_DRAW_BEGIN, 1);
   PlotIndexSetInteger(3, PLOT_DRAW_BEGIN, 1);
   PlotIndexSetInteger(4, PLOT_DRAW_BEGIN, 1);
   IndicatorSetString(INDICATOR_SHORTNAME, "BT_Pivots");
   return INIT_SUCCEEDED;
}

int OnCalculate(const int rates_total, const int prev_calculated,
                const datetime &time[], const double &open[],
                const double &high[], const double &low[], const double &close[],
                const long &tick_volume[], const long &volume[], const int &spread[])
{
   // Use H1 data to compute daily pivots
   // Simple approach: use previous bar's HLC for pivot calculation
   // For intrabar pivots, use previous bar H, L, C
   int start = (prev_calculated > 1) ? prev_calculated - 1 : 1;
   for(int i = start; i < rates_total; i++)
   {
      double ph = high[i-1], pl = low[i-1], pc = close[i-1];
      double pp = (ph + pl + pc) / 3.0;
      double rng = ph - pl;
      PPBuffer[i] = pp;
      R1Buffer[i] = 2 * pp - pl;
      S1Buffer[i] = 2 * pp - ph;
      R2Buffer[i] = pp + rng;
      S2Buffer[i] = pp - rng;
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
                        time_field: None,
                        candle_pattern: None,
                        offset: None,
                        compound_left: None,
                        compound_op: None,
                        compound_right: None,
                    },
                    comparator: Comparator::CrossAbove,
                    right_operand: Operand {
                        operand_type: OperandType::Indicator,
                        indicator: Some(IndicatorConfig {
                            indicator_type: IndicatorType::SMA,
                            params: IndicatorParams { period: Some(20), ..Default::default() },
                            output_field: None,
                            cached_hash: 0,
                        }),
                        price_field: None,
                        constant_value: None,
                        time_field: None,
                        candle_pattern: None,
                        offset: None,
                        compound_left: None,
                        compound_op: None,
                        compound_right: None,
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
                            cached_hash: 0,
                        }),
                        price_field: None,
                        constant_value: None,
                        time_field: None,
                        candle_pattern: None,
                        offset: None,
                        compound_left: None,
                        compound_op: None,
                        compound_right: None,
                    },
                    comparator: Comparator::GreaterThan,
                    right_operand: Operand {
                        operand_type: OperandType::Constant,
                        constant_value: Some(50.0),
                        indicator: None,
                        price_field: None,
                        time_field: None,
                        candle_pattern: None,
                        offset: None,
                        compound_left: None,
                        compound_op: None,
                        compound_right: None,
                    },
                    logical_operator: None,
                },
            ],
            short_entry_rules: vec![],
            long_exit_rules: vec![],
            short_exit_rules: vec![],
            long_entry_groups: vec![],
            short_entry_groups: vec![],
            long_exit_groups: vec![],
            short_exit_groups: vec![],
            position_sizing: PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: 0.1,
                decrease_factor: 0.9,
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
            entry_order: OrderType::Market,
            entry_order_offset_pips: 0.0,
            close_after_bars: None,
            move_sl_to_be: false,
            entry_order_indicator: None,
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
                    cached_hash: 0,
                }),
                price_field: None,
                constant_value: None,
                time_field: None,
                candle_pattern: None,
                offset: None,
                compound_left: None,
                compound_op: None,
                compound_right: None,
            },
            comparator: Comparator::GreaterThan,
            right_operand: Operand {
                operand_type: OperandType::Constant,
                constant_value: Some(0.0),
                indicator: None,
                price_field: None,
                time_field: None,
                candle_pattern: None,
                offset: None,
                compound_left: None,
                compound_op: None,
                compound_right: None,
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

// ══════════════════════════════════════════════════════════════
// SR MQL5 code generation
// ══════════════════════════════════════════════════════════════

/// Generate an MQL5 Expert Advisor from a Symbolic Regression strategy.
/// The EA evaluates the three formula trees at each new bar using BT_* custom
/// indicators and applies the configured position sizing, SL, TP, and costs.
pub fn generate_sr_mql5(
    strategy: &crate::models::sr_result::SrStrategy,
    name: &str,
) -> Result<CodeGenerationResult, AppError> {
    use crate::models::sr_result::{BinaryOpType, SrNode, UnaryOpType};
    use crate::models::strategy::{
        PositionSizingType, StopLossType, TakeProfitType, TradeDirection, TrailingStopType,
    };
    use std::collections::{HashMap, HashSet};

    let ea_name = name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");

    // ── 1. Collect unique indicator leaves ────────────────────────────────────
    fn collect_leaves(node: &SrNode, seen: &mut HashMap<String, IndicatorConfig>) {
        match node {
            SrNode::Constant(_) => {}
            SrNode::IndicatorLeaf { config, .. } => {
                seen.entry(config.cache_key()).or_insert_with(|| config.clone());
            }
            SrNode::BinaryOp { left, right, .. } => {
                collect_leaves(left, seen);
                collect_leaves(right, seen);
            }
            SrNode::UnaryOp { child, .. } => collect_leaves(child, seen),
        }
    }

    let mut leaf_map: HashMap<String, IndicatorConfig> = HashMap::new();
    collect_leaves(&strategy.entry_long, &mut leaf_map);
    collect_leaves(&strategy.entry_short, &mut leaf_map);
    collect_leaves(&strategy.exit, &mut leaf_map);

    // Sort for deterministic output
    let mut leaves: Vec<(String, IndicatorConfig)> = leaf_map.into_iter().collect();
    leaves.sort_by(|a, b| a.0.cmp(&b.0));

    // Build var_name map: cache_key → var_name
    let var_map: HashMap<String, String> = leaves.iter()
        .map(|(key, cfg)| (key.clone(), indicator_var_name(cfg)))
        .collect();

    // ── 2. Recursive MQL5 expression compiler ─────────────────────────────────
    fn node_to_mql5(
        node: &SrNode,
        var_map: &HashMap<String, String>,
        buffer_map: &HashMap<String, usize>,
    ) -> String {
        match node {
            SrNode::Constant(v) => format!("{:.6}", v),
            SrNode::IndicatorLeaf { config, buffer_index } => {
                let key = config.cache_key();
                let var = var_map.get(&key).map(|s| s.as_str()).unwrap_or("__unknown");
                let buf = buffer_map.get(&format!("{}_{}", key, buffer_index)).copied().unwrap_or(*buffer_index);
                format!("g_{}_{}", var, buf)
            }
            SrNode::BinaryOp { op, left, right } => {
                let l = node_to_mql5(left, var_map, buffer_map);
                let r = node_to_mql5(right, var_map, buffer_map);
                match op {
                    BinaryOpType::Add => format!("({} + {})", l, r),
                    BinaryOpType::Sub => format!("({} - {})", l, r),
                    BinaryOpType::Mul => format!("({} * {})", l, r),
                    BinaryOpType::ProtectedDiv => format!(
                        "(MathAbs({r}) < 1e-10 ? 0.0 : ({l}) / ({r}))",
                        l = l, r = r
                    ),
                }
            }
            SrNode::UnaryOp { op, child } => {
                let c = node_to_mql5(child, var_map, buffer_map);
                match op {
                    UnaryOpType::Sqrt => format!("MathSqrt(MathAbs({}))", c),
                    UnaryOpType::Abs  => format!("MathAbs({})", c),
                    UnaryOpType::Log  => format!("MathLog(MathAbs({}) + 1e-10)", c),
                    UnaryOpType::Neg  => format!("(-({}))", c),
                }
            }
        }
    }

    // Collect which (cache_key, buffer_index) pairs are actually used
    fn collect_buffers_used(node: &SrNode, out: &mut Vec<(String, usize)>) {
        match node {
            SrNode::Constant(_) => {}
            SrNode::IndicatorLeaf { config, buffer_index } => {
                out.push((config.cache_key(), *buffer_index));
            }
            SrNode::BinaryOp { left, right, .. } => {
                collect_buffers_used(left, out);
                collect_buffers_used(right, out);
            }
            SrNode::UnaryOp { child, .. } => collect_buffers_used(child, out),
        }
    }
    let mut used_buffers: Vec<(String, usize)> = Vec::new();
    collect_buffers_used(&strategy.entry_long, &mut used_buffers);
    collect_buffers_used(&strategy.entry_short, &mut used_buffers);
    collect_buffers_used(&strategy.exit, &mut used_buffers);
    used_buffers.sort();
    used_buffers.dedup();

    let buffer_map: HashMap<String, usize> = used_buffers.iter()
        .map(|(key, buf)| (format!("{}_{}", key, buf), *buf))
        .collect();

    let expr_long  = node_to_mql5(&strategy.entry_long,  &var_map, &buffer_map);
    let expr_short = node_to_mql5(&strategy.entry_short, &var_map, &buffer_map);
    let expr_exit  = node_to_mql5(&strategy.exit,        &var_map, &buffer_map);

    // ── 3. Determine which ATR handles are needed for SL / TP / TS ───────────
    //
    // The SR backtester pre-computes ATR-14 for all SL/TP regardless of atr_period.
    // We match that behaviour: always use period 14 for the SL/TP ATR handle unless
    // the field specifies a different period.
    let mut atr_periods_needed: HashSet<usize> = HashSet::new();
    if let Some(sl) = &strategy.stop_loss {
        if sl.sl_type == StopLossType::ATR {
            atr_periods_needed.insert(sl.atr_period.unwrap_or(14));
        }
    }
    if let Some(tp) = &strategy.take_profit {
        if tp.tp_type == TakeProfitType::ATR {
            atr_periods_needed.insert(tp.atr_period.unwrap_or(14));
        }
    }
    if let Some(ts) = &strategy.trailing_stop {
        if ts.ts_type == TrailingStopType::ATR {
            atr_periods_needed.insert(ts.atr_period.unwrap_or(14));
        }
    }
    let mut atr_periods: Vec<usize> = atr_periods_needed.into_iter().collect();
    atr_periods.sort();

    let need_trailing = strategy.trailing_stop.is_some();
    let allow_long  = !matches!(strategy.trade_direction, TradeDirection::Short);
    let allow_short = !matches!(strategy.trade_direction, TradeDirection::Long);

    // ── 4. Generate EA code ───────────────────────────────────────────────────
    let mut out = String::with_capacity(8192);

    // Header
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "//| SR Strategy EA: {:<48}|", ea_name).ok();
    writeln!(out, "//| Generated by Backtester SR Builder                               |").ok();
    writeln!(out, "//|                                                                  |").ok();
    writeln!(out, "//| Entry Long  condition : formula > InpLongThreshold               |").ok();
    writeln!(out, "//| Entry Short condition : formula < InpShortThreshold              |").ok();
    writeln!(out, "//| Exit condition        : exit formula changes sign                |").ok();
    writeln!(out, "//|   (exit guard: no exit on the bar immediately after entry)       |").ok();
    writeln!(out, "//| Signals read at shift=1 (previous completed bar) to match       |").ok();
    writeln!(out, "//| the backtester CopyBuffer(shift=1) behaviour exactly.            |").ok();
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "#property copyright \"Backtester SR Builder\"").ok();
    writeln!(out, "#property version   \"2.00\"").ok();
    writeln!(out, "#include <Trade\\Trade.mqh>").ok();
    writeln!(out).ok();

    // ── Inputs ────────────────────────────────────────────────────────────────
    writeln!(out, "// ── Entry / exit thresholds (evolved by SR — edit with care) ──────").ok();
    writeln!(out, "input double InpLongThreshold  = {:.6}; // Entry Long:  formula > this", strategy.long_threshold).ok();
    writeln!(out, "input double InpShortThreshold = {:.6}; // Entry Short: formula < this", strategy.short_threshold).ok();
    writeln!(out).ok();
    writeln!(out, "// ── Trade management ──────────────────────────────────────────────").ok();
    writeln!(out, "input int    InpMagicNumber = 88888;  // Magic number").ok();

    // Position sizing inputs
    match &strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots => {
            writeln!(out, "input double InpLotSize     = {:.2};  // Fixed lot size", strategy.position_sizing.value).ok();
        }
        PositionSizingType::PercentEquity | PositionSizingType::RiskBased => {
            writeln!(out, "input double InpRiskPct     = {:.2};  // % of equity to risk per trade", strategy.position_sizing.value).ok();
        }
        PositionSizingType::FixedAmount => {
            writeln!(out, "input double InpFixedAmount = {:.2};  // Fixed dollar amount to risk per trade", strategy.position_sizing.value).ok();
        }
        PositionSizingType::AntiMartingale => {
            writeln!(out, "input double InpRiskPct         = {:.2};  // % of equity to risk per trade", strategy.position_sizing.value).ok();
            writeln!(out, "input double InpDecreaseFactor  = {:.2};  // Lot multiplier after each loss (0-1)", strategy.position_sizing.decrease_factor).ok();
        }
    }

    // SL inputs
    if let Some(sl) = &strategy.stop_loss {
        match sl.sl_type {
            StopLossType::Pips       => { writeln!(out, "input double InpSLPips    = {:.1};  // Stop Loss in pips", sl.value).ok(); }
            StopLossType::Percentage => { writeln!(out, "input double InpSLPct     = {:.4};  // Stop Loss as % of price", sl.value).ok(); }
            StopLossType::ATR        => { writeln!(out, "input double InpSLAtrMult = {:.2};  // Stop Loss ATR multiplier", sl.value).ok(); }
        }
    }

    // TP inputs
    if let Some(tp) = &strategy.take_profit {
        match tp.tp_type {
            TakeProfitType::Pips        => { writeln!(out, "input double InpTPPips    = {:.1};  // Take Profit in pips", tp.value).ok(); }
            TakeProfitType::RiskReward  => { writeln!(out, "input double InpTPRR      = {:.2};  // Take Profit risk:reward ratio", tp.value).ok(); }
            TakeProfitType::ATR         => { writeln!(out, "input double InpTPAtrMult = {:.2};  // Take Profit ATR multiplier", tp.value).ok(); }
        }
    }

    // Trailing stop inputs
    if let Some(ts) = &strategy.trailing_stop {
        match ts.ts_type {
            TrailingStopType::ATR        => { writeln!(out, "input double InpTSAtrMult = {:.2};  // Trailing Stop ATR multiplier", ts.value).ok(); }
            TrailingStopType::RiskReward => { writeln!(out, "input double InpTSRR      = {:.2};  // Trailing Stop risk:reward ratio", ts.value).ok(); }
        }
    }
    writeln!(out).ok();

    // Indicator parameter inputs
    if !leaves.is_empty() {
        writeln!(out, "// ── SR indicator parameters (values fixed by SR evolution) ────────").ok();
        for (_, cfg) in &leaves {
            let v = indicator_var_name(cfg);
            if let Some(p) = cfg.params.period          { writeln!(out, "input int    Inp_{v}_period = {p};").ok(); }
            if let Some(p) = cfg.params.fast_period     { writeln!(out, "input int    Inp_{v}_fast   = {p};").ok(); }
            if let Some(p) = cfg.params.slow_period     { writeln!(out, "input int    Inp_{v}_slow   = {p};").ok(); }
            if let Some(p) = cfg.params.signal_period   { writeln!(out, "input int    Inp_{v}_signal = {p};").ok(); }
            if let Some(p) = cfg.params.k_period        { writeln!(out, "input int    Inp_{v}_k      = {p};").ok(); }
            if let Some(p) = cfg.params.d_period        { writeln!(out, "input int    Inp_{v}_d      = {p};").ok(); }
            if let Some(x) = cfg.params.std_dev         { writeln!(out, "input double Inp_{v}_stddev = {x};").ok(); }
            if let Some(x) = cfg.params.acceleration_factor { writeln!(out, "input double Inp_{v}_af  = {x};").ok(); }
            if let Some(x) = cfg.params.maximum_factor  { writeln!(out, "input double Inp_{v}_max    = {x};").ok(); }
            if let Some(x) = cfg.params.gamma           { writeln!(out, "input double Inp_{v}_gamma  = {x};").ok(); }
            if let Some(x) = cfg.params.multiplier      { writeln!(out, "input double Inp_{v}_mult   = {x};").ok(); }
        }
        writeln!(out).ok();
    }

    // ── Globals ───────────────────────────────────────────────────────────────
    writeln!(out, "// ── Global handles & state ────────────────────────────────────────").ok();
    writeln!(out, "CTrade g_trade;").ok();
    for (_, cfg) in &leaves {
        let v = indicator_var_name(cfg);
        writeln!(out, "int    g_{v}_handle  = INVALID_HANDLE;").ok();
    }
    // ATR handles for SL/TP/TS
    for &period in &atr_periods {
        writeln!(out, "int    g_atr{period}_handle = INVALID_HANDLE;  // ATR({period}) for SL/TP/TS").ok();
    }
    // Indicator value buffers (read once per bar)
    for (key, buf) in &used_buffers {
        if let Some(v) = var_map.get(key) {
            writeln!(out, "double g_{v}_{buf} = 0.0;").ok();
        }
    }
    // ATR value buffers
    for &period in &atr_periods {
        writeln!(out, "double g_atr{period}   = 0.0;  // ATR({period}) value (shift=1)").ok();
    }
    // Exit sign-change state
    writeln!(out, "double   g_exit_prev      = 0.0;    // exit formula value on previous bar").ok();
    // Entry bar time (for exit guard: no exit within 1 bar of entry)
    writeln!(out, "datetime g_entry_bar_time = 0;      // bar open time when position was opened").ok();
    // AntiMartingale consecutive loss counter
    if matches!(strategy.position_sizing.sizing_type, PositionSizingType::AntiMartingale) {
        writeln!(out, "int      g_consec_losses  = 0;      // consecutive losses for AntiMartingale sizing").ok();
    }
    writeln!(out).ok();

    // ── OnInit ────────────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "int OnInit()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   g_trade.SetExpertMagicNumber(InpMagicNumber);").ok();
    writeln!(out).ok();
    writeln!(out, "   // Create SR indicator handles").ok();
    for (_, cfg) in &leaves {
        let v = indicator_var_name(cfg);
        let call = sr_icustom_call(cfg, &v);
        writeln!(out, "   g_{v}_handle = {call};").ok();
        writeln!(out, "   if(g_{v}_handle == INVALID_HANDLE) {{ Print(\"ERROR: failed to create handle for {v}\"); return INIT_FAILED; }}").ok();
    }
    if !atr_periods.is_empty() {
        writeln!(out).ok();
        writeln!(out, "   // Create ATR handles for SL/TP/TS").ok();
    }
    for &period in &atr_periods {
        writeln!(out, "   g_atr{period}_handle = iATR(_Symbol, PERIOD_CURRENT, {period});").ok();
        writeln!(out, "   if(g_atr{period}_handle == INVALID_HANDLE) {{ Print(\"ERROR: failed to create ATR({period}) handle\"); return INIT_FAILED; }}").ok();
    }
    writeln!(out).ok();
    writeln!(out, "   // Reset state").ok();
    writeln!(out, "   g_exit_prev      = 0.0;").ok();
    writeln!(out, "   g_entry_bar_time = 0;").ok();
    writeln!(out, "   return INIT_SUCCEEDED;").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── OnDeinit ──────────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void OnDeinit(const int reason)").ok();
    writeln!(out, "{{").ok();
    for (_, cfg) in &leaves {
        let v = indicator_var_name(cfg);
        writeln!(out, "   if(g_{v}_handle != INVALID_HANDLE) IndicatorRelease(g_{v}_handle);").ok();
    }
    for &period in &atr_periods {
        writeln!(out, "   if(g_atr{period}_handle != INVALID_HANDLE) IndicatorRelease(g_atr{period}_handle);").ok();
    }
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── OnTick ────────────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void OnTick()").ok();
    writeln!(out, "{{").ok();

    // New-bar guard — all logic runs once per bar at bar open
    writeln!(out, "   // Execute only once per bar (bar-open logic, matching backtester)").ok();
    writeln!(out, "   static datetime s_prev_bar = 0;").ok();
    writeln!(out, "   datetime cur_bar = iTime(_Symbol, PERIOD_CURRENT, 0);").ok();
    writeln!(out, "   if(cur_bar == 0 || cur_bar == s_prev_bar) return;").ok();
    writeln!(out, "   s_prev_bar = cur_bar;").ok();
    writeln!(out).ok();

    // Read indicator buffers at shift=1 (previous completed bar)
    writeln!(out, "   // ── Read indicator values at shift=1 (previous bar, matching backtester) ──").ok();
    writeln!(out, "   double _b[];").ok();
    for (key, buf) in &used_buffers {
        if let Some(v) = var_map.get(key) {
            writeln!(out, "   if(CopyBuffer(g_{v}_handle, {buf}, 1, 1, _b) == 1) g_{v}_{buf} = _b[0]; else {{ Print(\"WARN: CopyBuffer failed for {v} buf={buf}\"); }}").ok();
        }
    }
    // Read ATR values at shift=1
    if !atr_periods.is_empty() {
        writeln!(out).ok();
        writeln!(out, "   // ── Read ATR values at shift=1 ───────────────────────────────────").ok();
        for &period in &atr_periods {
            writeln!(out, "   if(CopyBuffer(g_atr{period}_handle, 0, 1, 1, _b) == 1) g_atr{period} = _b[0];").ok();
        }
    }
    writeln!(out).ok();

    // Evaluate SR formulas
    writeln!(out, "   // ── Evaluate SR formulas ──────────────────────────────────────────").ok();
    writeln!(out, "   double signal_long  = {expr_long};").ok();
    writeln!(out, "   double signal_short = {expr_short};").ok();
    writeln!(out, "   double signal_exit  = {expr_exit};").ok();
    writeln!(out).ok();

    // Exit sign-change detection
    // Matches runner.rs: (prev >= 0 && cur < 0) || (prev <= 0 && cur > 0)
    // Exit guard: i > entry_bar + 1 → in MQL5: the bar before current (shift=1) must NOT be the entry bar.
    writeln!(out, "   // ── Exit logic ────────────────────────────────────────────────────").ok();
    writeln!(out, "   // Sign change in exit formula triggers close (matches backtester runner.rs).").ok();
    writeln!(out, "   bool sign_changed = (g_exit_prev >= 0.0 && signal_exit < 0.0)").ok();
    writeln!(out, "                    || (g_exit_prev <= 0.0 && signal_exit > 0.0);").ok();
    writeln!(out, "   // Exit guard: do NOT exit on the bar immediately after entry").ok();
    writeln!(out, "   // (matches condition: i > pos.entry_bar + 1 in the backtester).").ok();
    writeln!(out, "   datetime prev_bar = iTime(_Symbol, PERIOD_CURRENT, 1);").ok();
    writeln!(out, "   bool exit_guard_ok = (g_entry_bar_time == 0 || prev_bar != g_entry_bar_time);").ok();
    writeln!(out, "   bool do_exit = sign_changed && exit_guard_ok;").ok();
    writeln!(out, "   g_exit_prev = signal_exit;").ok();
    writeln!(out).ok();

    // Manage open positions: handle exit signal + trailing stop
    writeln!(out, "   bool has_long  = false;").ok();
    writeln!(out, "   bool has_short = false;").ok();
    writeln!(out, "   for(int _i = PositionsTotal() - 1; _i >= 0; _i--)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      ulong ticket = PositionGetTicket(_i);").ok();
    writeln!(out, "      if(ticket == 0) continue;").ok();
    writeln!(out, "      if((long)PositionGetInteger(POSITION_MAGIC) != InpMagicNumber) continue;").ok();
    writeln!(out, "      if(PositionGetString(POSITION_SYMBOL) != _Symbol) continue;").ok();
    writeln!(out, "      ENUM_POSITION_TYPE ptype = (ENUM_POSITION_TYPE)PositionGetInteger(POSITION_TYPE);").ok();
    writeln!(out, "      if(ptype == POSITION_TYPE_BUY)  has_long  = true;").ok();
    writeln!(out, "      if(ptype == POSITION_TYPE_SELL) has_short = true;").ok();
    writeln!(out, "      if(do_exit) g_trade.PositionClose(ticket);").ok();
    writeln!(out, "   }}").ok();
    writeln!(out).ok();

    // Trailing stop update (after exit check, before entry)
    if need_trailing {
        writeln!(out, "   // ── Trailing stop ─────────────────────────────────────────────────").ok();
        writeln!(out, "   if(!do_exit && (has_long || has_short)) SR_ManageTrailingStop();").ok();
        writeln!(out).ok();
    }

    // Entry logic
    writeln!(out, "   // ── Entry logic ───────────────────────────────────────────────────").ok();
    writeln!(out, "   // If exit just fired, skip entry this bar (same as backtester).").ok();
    writeln!(out, "   if(!do_exit && !has_long && !has_short)").ok();
    writeln!(out, "   {{").ok();

    if allow_long && allow_short {
        // Both: long takes priority when both fire
        writeln!(out, "      bool go_long  = (signal_long  >  InpLongThreshold)  && MathIsValidNumber(signal_long);").ok();
        writeln!(out, "      bool go_short = (signal_short <  InpShortThreshold) && MathIsValidNumber(signal_short);").ok();
        writeln!(out).ok();
        writeln!(out, "      if(go_long)  // Long takes priority when both fire (matches backtester)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_ASK);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_BUY, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_BUY, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Buy(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        writeln!(out, "      }}").ok();
        writeln!(out, "      else if(go_short)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_BID);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_SELL, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_SELL, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Sell(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        writeln!(out, "      }}").ok();
    } else if allow_long {
        writeln!(out, "      if((signal_long > InpLongThreshold) && MathIsValidNumber(signal_long))").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_ASK);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_BUY, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_BUY, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Buy(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        writeln!(out, "      }}").ok();
    } else if allow_short {
        writeln!(out, "      if((signal_short < InpShortThreshold) && MathIsValidNumber(signal_short))").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_BID);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_SELL, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_SELL, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Sell(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        writeln!(out, "      }}").ok();
    }

    writeln!(out, "   }} // end entry block").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── SR_CalcLots ──────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "// SR_CalcLots: mirrors position.rs calculate_lots()").ok();
    writeln!(out, "// price = entry price, sl = stop loss price (0 = no SL)").ok();
    writeln!(out, "double SR_CalcLots(double price, double sl)").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   double minLot  = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MIN);").ok();
    writeln!(out, "   double maxLot  = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_MAX);").ok();
    writeln!(out, "   double step    = SymbolInfoDouble(_Symbol, SYMBOL_VOLUME_STEP);").ok();
    writeln!(out, "   if(step <= 0.0) step = minLot;").ok();

    match &strategy.position_sizing.sizing_type {
        PositionSizingType::FixedLots => {
            writeln!(out, "   double lots = InpLotSize;").ok();
        }
        PositionSizingType::FixedAmount => {
            writeln!(out, "   // Fixed-amount risk: if no SL fall back to min lot").ok();
            writeln!(out, "   if(sl == 0.0) return minLot;").ok();
            writeln!(out, "   double tickVal  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(tickVal <= 0.0 || tickSize <= 0.0) return minLot;").ok();
            writeln!(out, "   double slMoney = (MathAbs(price - sl) / tickSize) * tickVal;").ok();
            writeln!(out, "   if(slMoney <= 0.0) return minLot;").ok();
            writeln!(out, "   double lots = InpFixedAmount / slMoney;").ok();
        }
        PositionSizingType::PercentEquity | PositionSizingType::RiskBased => {
            writeln!(out, "   // Risk-based sizing: equity * pct% / SL monetary distance per lot").ok();
            writeln!(out, "   if(sl == 0.0) return minLot;").ok();
            writeln!(out, "   double equity   = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickVal  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(tickVal <= 0.0 || tickSize <= 0.0) return minLot;").ok();
            writeln!(out, "   double riskAmt  = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slMoney  = (MathAbs(price - sl) / tickSize) * tickVal;").ok();
            writeln!(out, "   if(slMoney <= 0.0) return minLot;").ok();
            writeln!(out, "   double lots = riskAmt / slMoney;").ok();
        }
        PositionSizingType::AntiMartingale => {
            writeln!(out, "   // AntiMartingale: risk-based with decay per consecutive loss").ok();
            writeln!(out, "   if(sl == 0.0) return minLot;").ok();
            writeln!(out, "   double equity   = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickVal  = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(tickVal <= 0.0 || tickSize <= 0.0) return minLot;").ok();
            writeln!(out, "   double riskAmt  = equity * InpRiskPct / 100.0;").ok();
            writeln!(out, "   double slMoney  = (MathAbs(price - sl) / tickSize) * tickVal;").ok();
            writeln!(out, "   if(slMoney <= 0.0) return minLot;").ok();
            writeln!(out, "   double decay    = MathPow(InpDecreaseFactor, g_consec_losses);").ok();
            writeln!(out, "   double lots = (riskAmt / slMoney) * decay;").ok();
        }
    }

    writeln!(out, "   lots = MathFloor(lots / step) * step;").ok();
    writeln!(out, "   lots = MathMax(minLot, MathMin(maxLot, lots));").ok();
    writeln!(out, "   return lots;").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── SR_CalcSL ─────────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "// SR_CalcSL: mirrors position.rs calculate_stop_loss()").ok();
    writeln!(out, "// Returns stop-loss price, or 0.0 if no SL configured.").ok();
    writeln!(out, "double SR_CalcSL(ENUM_ORDER_TYPE dir, double price)").ok();
    writeln!(out, "{{").ok();
    if let Some(sl) = &strategy.stop_loss {
        let period = sl.atr_period.unwrap_or(14);
        match sl.sl_type {
            StopLossType::Pips => {
                // pip = _Point * 10 for 5-digit brokers; this matches pip_size = 0.0001
                writeln!(out, "   double dist = InpSLPips * _Point * 10.0;  // pips → price distance").ok();
                writeln!(out, "   double sl   = (dir == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
                writeln!(out, "   return NormalizeDouble(sl, _Digits);").ok();
            }
            StopLossType::Percentage => {
                writeln!(out, "   double dist = price * InpSLPct / 100.0;").ok();
                writeln!(out, "   double sl   = (dir == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
                writeln!(out, "   return NormalizeDouble(sl, _Digits);").ok();
            }
            StopLossType::ATR => {
                writeln!(out, "   if(g_atr{period} <= 0.0) return 0.0;  // ATR not ready yet").ok();
                writeln!(out, "   double dist = g_atr{period} * InpSLAtrMult;").ok();
                writeln!(out, "   double sl   = (dir == ORDER_TYPE_BUY) ? price - dist : price + dist;").ok();
                writeln!(out, "   return NormalizeDouble(sl, _Digits);").ok();
            }
        }
    } else {
        writeln!(out, "   return 0.0;  // No stop loss configured").ok();
    }
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── SR_CalcTP ─────────────────────────────────────────────────────────────
    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "// SR_CalcTP: mirrors position.rs calculate_take_profit()").ok();
    writeln!(out, "// sl = stop-loss price (used for RiskReward TP); 0.0 = no SL.").ok();
    writeln!(out, "double SR_CalcTP(ENUM_ORDER_TYPE dir, double price, double sl)").ok();
    writeln!(out, "{{").ok();
    if let Some(tp) = &strategy.take_profit {
        let period = tp.atr_period.unwrap_or(14);
        match tp.tp_type {
            TakeProfitType::Pips => {
                writeln!(out, "   double dist = InpTPPips * _Point * 10.0;").ok();
                writeln!(out, "   double tp   = (dir == ORDER_TYPE_BUY) ? price + dist : price - dist;").ok();
                writeln!(out, "   return NormalizeDouble(tp, _Digits);").ok();
            }
            TakeProfitType::RiskReward => {
                writeln!(out, "   if(sl == 0.0) return 0.0;  // need SL for R:R TP").ok();
                writeln!(out, "   double slDist = MathAbs(price - sl);").ok();
                writeln!(out, "   double dist   = slDist * InpTPRR;").ok();
                writeln!(out, "   double tp     = (dir == ORDER_TYPE_BUY) ? price + dist : price - dist;").ok();
                writeln!(out, "   return NormalizeDouble(tp, _Digits);").ok();
            }
            TakeProfitType::ATR => {
                writeln!(out, "   if(g_atr{period} <= 0.0) return 0.0;  // ATR not ready yet").ok();
                writeln!(out, "   double dist = g_atr{period} * InpTPAtrMult;").ok();
                writeln!(out, "   double tp   = (dir == ORDER_TYPE_BUY) ? price + dist : price - dist;").ok();
                writeln!(out, "   return NormalizeDouble(tp, _Digits);").ok();
            }
        }
    } else {
        writeln!(out, "   return 0.0;  // No take profit configured").ok();
    }
    writeln!(out, "}}").ok();
    writeln!(out).ok();

    // ── SR_ManageTrailingStop ─────────────────────────────────────────────────
    if let Some(ts) = &strategy.trailing_stop {
        let ts_period = ts.atr_period.unwrap_or(14);
        writeln!(out, "//+------------------------------------------------------------------+").ok();
        writeln!(out, "// SR_ManageTrailingStop: mirrors position.rs update_trailing_stop()").ok();
        writeln!(out, "void SR_ManageTrailingStop()").ok();
        writeln!(out, "{{").ok();
        writeln!(out, "   for(int _i = PositionsTotal() - 1; _i >= 0; _i--)").ok();
        writeln!(out, "   {{").ok();
        writeln!(out, "      ulong ticket = PositionGetTicket(_i);").ok();
        writeln!(out, "      if(ticket == 0) continue;").ok();
        writeln!(out, "      if((long)PositionGetInteger(POSITION_MAGIC) != InpMagicNumber) continue;").ok();
        writeln!(out, "      if(PositionGetString(POSITION_SYMBOL) != _Symbol) continue;").ok();
        writeln!(out).ok();
        writeln!(out, "      double curSL    = PositionGetDouble(POSITION_SL);").ok();
        writeln!(out, "      double curTP    = PositionGetDouble(POSITION_TP);").ok();
        writeln!(out, "      double entry    = PositionGetDouble(POSITION_PRICE_OPEN);").ok();
        writeln!(out, "      ENUM_POSITION_TYPE ptype = (ENUM_POSITION_TYPE)PositionGetInteger(POSITION_TYPE);").ok();
        writeln!(out).ok();

        match ts.ts_type {
            TrailingStopType::ATR => {
                writeln!(out, "      // ATR trailing stop").ok();
                writeln!(out, "      if(g_atr{ts_period} <= 0.0) continue;").ok();
                writeln!(out, "      double trailDist = g_atr{ts_period} * InpTSAtrMult;").ok();
            }
            TrailingStopType::RiskReward => {
                writeln!(out, "      // Risk:Reward trailing stop (distance = initial SL distance * ratio)").ok();
                writeln!(out, "      if(curSL == 0.0) continue;").ok();
                writeln!(out, "      double trailDist = MathAbs(entry - curSL) * InpTSRR;").ok();
            }
        }

        writeln!(out).ok();
        writeln!(out, "      if(ptype == POSITION_TYPE_BUY)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double newSL = NormalizeDouble(SymbolInfoDouble(_Symbol, SYMBOL_BID) - trailDist, _Digits);").ok();
        writeln!(out, "         // Only move SL upward and only if it improves on current SL").ok();
        writeln!(out, "         if(newSL > curSL + _Point && newSL < SymbolInfoDouble(_Symbol, SYMBOL_BID))").ok();
        writeln!(out, "            g_trade.PositionModify(_Symbol, newSL, curTP);").ok();
        writeln!(out, "      }}").ok();
        writeln!(out, "      else if(ptype == POSITION_TYPE_SELL)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double newSL = NormalizeDouble(SymbolInfoDouble(_Symbol, SYMBOL_ASK) + trailDist, _Digits);").ok();
        writeln!(out, "         // Only move SL downward and only if it improves on current SL").ok();
        writeln!(out, "         if((curSL == 0.0 || newSL < curSL - _Point) && newSL > SymbolInfoDouble(_Symbol, SYMBOL_ASK))").ok();
        writeln!(out, "            g_trade.PositionModify(_Symbol, newSL, curTP);").ok();
        writeln!(out, "      }}").ok();
        writeln!(out, "   }}").ok();
        writeln!(out, "}}").ok();
        writeln!(out).ok();
    }

    // ── OnTradeTransaction (AntiMartingale loss counter) ──────────────────────
    if matches!(strategy.position_sizing.sizing_type, PositionSizingType::AntiMartingale) {
        writeln!(out, "//+------------------------------------------------------------------+").ok();
        writeln!(out, "// Track consecutive losses for AntiMartingale position sizing.").ok();
        writeln!(out, "void OnTradeTransaction(const MqlTradeTransaction &trans,").ok();
        writeln!(out, "                        const MqlTradeRequest &req,").ok();
        writeln!(out, "                        const MqlTradeResult  &res)").ok();
        writeln!(out, "{{").ok();
        writeln!(out, "   if(trans.type != TRADE_TRANSACTION_DEAL_ADD) return;").ok();
        writeln!(out, "   if((long)HistoryDealGetInteger(trans.deal, DEAL_MAGIC) != InpMagicNumber) return;").ok();
        writeln!(out, "   if(HistoryDealGetString(trans.deal, DEAL_SYMBOL) != _Symbol) return;").ok();
        writeln!(out, "   ENUM_DEAL_ENTRY entry = (ENUM_DEAL_ENTRY)HistoryDealGetInteger(trans.deal, DEAL_ENTRY);").ok();
        writeln!(out, "   if(entry != DEAL_ENTRY_OUT) return;").ok();
        writeln!(out, "   double profit = HistoryDealGetDouble(trans.deal, DEAL_PROFIT)").ok();
        writeln!(out, "                 + HistoryDealGetDouble(trans.deal, DEAL_SWAP)").ok();
        writeln!(out, "                 + HistoryDealGetDouble(trans.deal, DEAL_COMMISSION);").ok();
        writeln!(out, "   if(profit < 0.0) g_consec_losses++;").ok();
        writeln!(out, "   else             g_consec_losses = 0;").ok();
        writeln!(out, "}}").ok();
        writeln!(out).ok();
    }

    Ok(CodeGenerationResult {
        files: vec![CodeFile {
            filename: format!("{}.mq5", ea_name),
            code: out,
            is_main: true,
        }],
    })
}

/// Build the `iCustom()` call string for an SR indicator leaf.
fn sr_icustom_call(cfg: &IndicatorConfig, var: &str) -> String {
    match cfg.indicator_type {
        IndicatorType::SMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_SMA\", Inp_{}_period)", var),
        IndicatorType::EMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_EMA\", Inp_{}_period)", var),
        IndicatorType::RSI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_RSI\", Inp_{}_period)", var),
        IndicatorType::MACD => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_MACD\", Inp_{0}_fast, Inp_{0}_slow, Inp_{0}_signal)", var),
        IndicatorType::BollingerBands => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_BollingerBands\", Inp_{0}_period, Inp_{0}_stddev)", var),
        IndicatorType::ATR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_ATR\", Inp_{}_period)", var),
        IndicatorType::Stochastic => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_Stochastic\", Inp_{0}_k, Inp_{0}_d)", var),
        IndicatorType::ADX => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_ADX\", Inp_{}_period)", var),
        IndicatorType::CCI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_CCI\", Inp_{}_period)", var),
        IndicatorType::WilliamsR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_WilliamsR\", Inp_{}_period)", var),
        IndicatorType::ParabolicSAR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_ParabolicSAR\", Inp_{0}_af, Inp_{0}_max)", var),
        IndicatorType::ROC => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_ROC\", Inp_{}_period)", var),
        IndicatorType::VWAP => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_VWAP\")".to_string(),
        IndicatorType::Ichimoku => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_Ichimoku\", Inp_{0}_tenkan, Inp_{0}_kijun, Inp_{0}_senkou)", var),
        IndicatorType::KeltnerChannel => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_KeltnerChannel\", Inp_{0}_period, Inp_{0}_mult)", var),
        IndicatorType::SuperTrend => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_SuperTrend\", Inp_{0}_period, Inp_{0}_mult)", var),
        IndicatorType::LaguerreRSI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_LaguerreRSI\", Inp_{}_gamma)", var),
        IndicatorType::BearsPower => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_BearsPower\", Inp_{}_period)", var),
        IndicatorType::BullsPower => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_BullsPower\", Inp_{}_period)", var),
        IndicatorType::DeMarker => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_DeMarker\", Inp_{}_period)", var),
        IndicatorType::AwesomeOscillator => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_AwesomeOscillator\")".to_string(),
        IndicatorType::BarRange => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BarRange\")".to_string(),
        IndicatorType::TrueRange => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_TrueRange\")".to_string(),
        IndicatorType::Momentum => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_Momentum\", Inp_{}_period)", var),
        IndicatorType::LinearRegression => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_LinearRegression\", Inp_{}_period)", var),
        IndicatorType::Fractal => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Fractal\")".to_string(),
        IndicatorType::StdDev => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_StdDev\", Inp_{}_period)", var),
        IndicatorType::HeikenAshi => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HeikenAshi\")".to_string(),
        _ => {
            let type_name = format!("{:?}", cfg.indicator_type);
            if cfg.params.period.is_some() {
                format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_{}\", Inp_{}_period)", type_name, var)
            } else {
                format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_{}\")", type_name)
            }
        }
    }
}
