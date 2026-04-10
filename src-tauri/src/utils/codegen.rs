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
    mql5_globals(&mut out, strategy, &indicators);
    mql5_on_init(&mut out, &indicators);
    mql5_on_deinit(&mut out, &indicators);
    mql5_on_tick(&mut out, strategy);
    mql5_check_rules_fn(&mut out, &strategy.long_entry_rules, &strategy.long_entry_groups, "CheckLongEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_entry_rules, &strategy.short_entry_groups, "CheckShortEntry", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.long_exit_rules, &strategy.long_exit_groups, "CheckLongExit", &indicators);
    mql5_check_rules_fn(&mut out, &strategy.short_exit_rules, &strategy.short_exit_groups, "CheckShortExit", &indicators);
    mql5_open_position(&mut out, true);
    mql5_open_position(&mut out, false);
    mql5_close_position(&mut out, strategy);
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
        IndicatorType::AvgVolume => "avgvol",
        IndicatorType::BBWidthRatio => "bbwr",
        IndicatorType::EfficiencyRatio => "er",
        IndicatorType::HighestIndex => "hi_idx",
        IndicatorType::KAMA => "kama",
        IndicatorType::LowestIndex => "lo_idx",
        IndicatorType::QQE => "qqe",
        IndicatorType::SchaffTrendCycle => "stc",
    };

    let mut s = String::from(name);

    // These indicators have no parameters in their MQL5 implementation — don't
    // append any suffix so duplicates are deduplicated and no phantom Inp_* vars are needed.
    let no_params = matches!(ind.indicator_type,
        IndicatorType::BarRange | IndicatorType::TrueRange |
        IndicatorType::AwesomeOscillator |
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
        IndicatorType::QQE => match field {
            "tr_level" | "trend_level" => 1,
            _ => 0, // "rsi_ma" or default
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
    writeln!(out, "// ═══════════════ BACKTESTER MATCHING GUIDE ═══════════════════════").ok();
    writeln!(out, "// Strategy Tester Mode : \"Open prices only\"").ok();
    writeln!(out, "//   (matches Backtester bar-close decision model)").ok();
    writeln!(out, "//   \"Every tick\"  → more SL/TP triggers → lower profit than Backtester").ok();
    writeln!(out, "//   \"OHLC on M1\" → intermediate; use if Backtester uses M1 simulation").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Commission  : InpCommission reflects the value configured in Backtester.").ok();
    writeln!(out, "//               Set InpCommission=0 if your broker already charges commission").ok();
    writeln!(out, "//               in Tester settings — otherwise it will be double-counted.").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Slippage    : InpSlippagePips adjusts entry/exit price to simulate slippage.").ok();
    writeln!(out, "//               Set to 0 if you prefer natural market execution.").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Data        : Use same symbol / timeframe / date range as Backtester.").ok();
    writeln!(out, "//               MT5 history server downloads bid prices ✓").ok();
    writeln!(out, "//               CSV from Yahoo/mid-price sources → spreads will differ.").ok();
    writeln!(out, "// ═══════════════════════════════════════════════════════════════════").ok();
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
            IndicatorType::ROC | IndicatorType::WilliamsR |
            IndicatorType::AvgVolume | IndicatorType::EfficiencyRatio |
            IndicatorType::HighestIndex | IndicatorType::LowestIndex => {
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
            IndicatorType::AwesomeOscillator |
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
            IndicatorType::BBWidthRatio => {
                writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, p.period.unwrap_or(20)).ok();
                writeln!(out, "input double Inp_{}_stddev = {:.1};", ind.var_name, p.std_dev.unwrap_or(2.0)).ok();
            }
            IndicatorType::KAMA => {
                writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, p.period.unwrap_or(10)).ok();
                writeln!(out, "input int    Inp_{}_fast = {};", ind.var_name, p.fast_period.unwrap_or(2)).ok();
                writeln!(out, "input int    Inp_{}_slow = {};", ind.var_name, p.slow_period.unwrap_or(30)).ok();
            }
            IndicatorType::QQE => {
                writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, p.period.unwrap_or(14)).ok();
                writeln!(out, "input int    Inp_{}_sf = {};", ind.var_name, p.signal_period.unwrap_or(5)).ok();
                writeln!(out, "input double Inp_{}_wf = {:.3};", ind.var_name, p.multiplier.unwrap_or(4.236)).ok();
            }
            IndicatorType::SchaffTrendCycle => {
                writeln!(out, "input int    Inp_{}_period = {};", ind.var_name, p.period.unwrap_or(10)).ok();
                writeln!(out, "input int    Inp_{}_fast = {};", ind.var_name, p.fast_period.unwrap_or(20)).ok();
                writeln!(out, "input int    Inp_{}_slow = {};", ind.var_name, p.slow_period.unwrap_or(50)).ok();
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
    if let Some(ct) = &strategy.close_trades_at {
        writeln!(out, "input int    InpCloseHour   = {};       // Force-close hour (0-23)", ct.hour).ok();
        writeln!(out, "input int    InpCloseMinute = {};       // Force-close minute (0-59)", ct.minute).ok();
    }
    if let Some(n) = strategy.close_after_bars {
        writeln!(out, "input int    InpCloseAfterBars = {};    // Close position after N bars", n).ok();
    }

    writeln!(out).ok();
}

fn mql5_globals(out: &mut String, strategy: &Strategy, indicators: &[UniqueIndicator]) {
    writeln!(out, "// ═══════════════ GLOBAL VARIABLES ═══════════════").ok();
    writeln!(out, "CTrade trade;").ok();
    for ind in indicators {
        writeln!(out, "int {};", ind.handle_name).ok();
    }
    writeln!(out, "int    g_dailyTradeCount    = 0;  // trades opened today").ok();
    writeln!(out, "int    g_lastTradeYYYYMMDD  = 0;  // date of last daily-counter reset (YYYYMMDD)").ok();
    if matches!(strategy.position_sizing.sizing_type, PositionSizingType::AntiMartingale) {
        writeln!(out, "int    g_consecLosses       = 0;  // consecutive losing trades (AntiMartingale)").ok();
    }
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
            // ── SQX indicators (use BT_* names — self-contained files generated alongside EA) ──
            IndicatorType::ATR => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ATR\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::Stochastic => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Stochastic\", Inp_{0}_k, Inp_{0}_d, 3, MODE_SMA, STO_LOWHIGH)",
                ind.var_name
            ),
            IndicatorType::ADX => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_ADX\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::CCI => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_CCI\", Inp_{}_period, PRICE_TYPICAL)",
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
            IndicatorType::Ichimoku => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Ichimoku\", Inp_{0}_tenkan, Inp_{0}_kijun, Inp_{0}_senkou)",
                ind.var_name
            ),
            IndicatorType::KeltnerChannel => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_KeltnerChannel\", Inp_{0}_period, Inp_{0}_mult)",
                ind.var_name
            ),
            IndicatorType::SuperTrend => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_SuperTrend\", 1, Inp_{0}_period, Inp_{0}_mult)",
                ind.var_name
            ),
            IndicatorType::LaguerreRSI => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LaguerreRSI\", Inp_{}_gamma)",
                ind.var_name
            ),
            IndicatorType::BearsPower => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BearsPower\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::BullsPower => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BullsPower\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::TrueRange => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_TrueRange\")"
            ),
            IndicatorType::LinearRegression => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LinearRegression\", Inp_{}_period, PRICE_CLOSE)",
                ind.var_name
            ),
            IndicatorType::Fractal => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Fractal\", 3)"
            ),
            IndicatorType::HeikenAshi => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HeikenAshi\")"
            ),
            IndicatorType::GannHiLo => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_GannHiLo\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::HullMA => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HullMA\", Inp_{}_period, 2.0, PRICE_CLOSE)",
                ind.var_name
            ),
            IndicatorType::UlcerIndex => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_UlcerIndex\", Inp_{}_period, 1)",
                ind.var_name
            ),
            IndicatorType::Vortex => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Vortex\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::Aroon => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Aroon\", Inp_{}_period, 0)",
                ind.var_name
            ),
            IndicatorType::HighestInRange => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HighestInRange\", Inp_{}_period, PRICE_HIGH)",
                ind.var_name
            ),
            IndicatorType::LowestInRange => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LowestInRange\", Inp_{}_period, PRICE_LOW)",
                ind.var_name
            ),
            IndicatorType::Reflex => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Reflex\", Inp_{}_period)",
                ind.var_name
            ),
            // ── New SQX indicators ──
            IndicatorType::AvgVolume => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_AvgVolume\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::BBWidthRatio => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BBWidthRatio\", Inp_{0}_period, Inp_{0}_stddev, PRICE_CLOSE)",
                ind.var_name
            ),
            IndicatorType::EfficiencyRatio => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_EfficiencyRatio\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::HighestIndex => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_HighestIndex\", Inp_{}_period, PRICE_HIGH)",
                ind.var_name
            ),
            IndicatorType::KAMA => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_KAMA\", Inp_{0}_period, Inp_{0}_fast, Inp_{0}_slow, 0)",
                ind.var_name
            ),
            IndicatorType::LowestIndex => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_LowestIndex\", Inp_{}_period, PRICE_LOW)",
                ind.var_name
            ),
            IndicatorType::QQE => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_QQE\", Inp_{0}_period, Inp_{0}_sf, Inp_{0}_wf)",
                ind.var_name
            ),
            IndicatorType::SchaffTrendCycle => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_SchaffTrendCycle\", Inp_{0}_period, Inp_{0}_fast, Inp_{0}_slow, 3.0)",
                ind.var_name
            ),
            // ── Non-SQX indicators (keep BT_* custom files) ──
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
            IndicatorType::Momentum => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_Momentum\", Inp_{}_period)",
                ind.var_name
            ),
            IndicatorType::StdDev => format!(
                "iCustom(_Symbol, PERIOD_CURRENT, \"BT_StdDev\", Inp_{}_period)",
                ind.var_name
            ),
            // --- Fallback for any remaining types ---
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
    let can_long  = strategy.trade_direction != TradeDirection::Short;
    let can_short = strategy.trade_direction != TradeDirection::Long;
    let has_long_entry  = has_rules(&strategy.long_entry_rules,  &strategy.long_entry_groups);
    let has_short_entry = has_rules(&strategy.short_entry_rules, &strategy.short_entry_groups);
    let has_long_exit   = has_rules(&strategy.long_exit_rules,   &strategy.long_exit_groups);
    let has_short_exit  = has_rules(&strategy.short_exit_rules,  &strategy.short_exit_groups);

    let needs_dt = strategy.trading_hours.is_some()
        || strategy.close_trades_at.is_some()
        || strategy.max_daily_trades.is_some();

    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void OnTick()").ok();
    writeln!(out, "{{").ok();

    // ── New-bar guard ───────────────────────────────────────────────────────────
    writeln!(out, "   // Execute logic once per completed bar").ok();
    writeln!(out, "   static datetime prevBarTime = 0;").ok();
    writeln!(out, "   datetime currentBarTime = iTime(_Symbol, PERIOD_CURRENT, 0);").ok();
    writeln!(out, "   if(currentBarTime == prevBarTime) return;").ok();
    writeln!(out, "   prevBarTime = currentBarTime;").ok();
    writeln!(out).ok();

    // ── Decode bar open time (once, if needed) ────────────────────────────────
    // Use currentBarTime (= iTime bar open), not TimeCurrent() (= live server clock).
    // This guarantees that time filters compare against the same timestamp the Rust
    // engine uses (candle.datetime = bar open time), keeping tester and live in sync.
    if needs_dt {
        writeln!(out, "   MqlDateTime dt;").ok();
        writeln!(out, "   TimeToStruct(currentBarTime, dt);").ok();
        writeln!(out).ok();
    }

    // ── Daily trade-count reset ─────────────────────────────────────────────────
    if strategy.max_daily_trades.is_some() {
        writeln!(out, "   // Reset daily trade counter at the start of each new calendar day").ok();
        writeln!(out, "   int todayYYYYMMDD = dt.year * 10000 + dt.mon * 100 + dt.day;").ok();
        writeln!(out, "   if(todayYYYYMMDD != g_lastTradeYYYYMMDD)").ok();
        writeln!(out, "   {{").ok();
        writeln!(out, "      g_lastTradeYYYYMMDD = todayYYYYMMDD;").ok();
        writeln!(out, "      g_dailyTradeCount   = 0;").ok();
        writeln!(out, "   }}").ok();
        writeln!(out).ok();
    }

    // ── Trading hours filter ────────────────────────────────────────────────────
    // Always declare inHours at OnTick scope so the entry block can use it safely.
    if strategy.trading_hours.is_some() {
        writeln!(out, "   // Trading hours filter — inHours=true means entries are allowed now").ok();
        writeln!(out, "   int cur_min   = dt.hour * 60 + dt.min;").ok();
        writeln!(out, "   int start_min = InpStartHour * 60 + InpStartMinute;").ok();
        writeln!(out, "   int end_min   = InpEndHour   * 60 + InpEndMinute;").ok();
        writeln!(out, "   bool inHours;").ok();
        writeln!(out, "   if(start_min <= end_min)").ok();
        writeln!(out, "      inHours = (cur_min >= start_min && cur_min <= end_min);").ok();
        writeln!(out, "   else // session crosses midnight").ok();
        writeln!(out, "      inHours = (cur_min >= start_min || cur_min <= end_min);").ok();
        writeln!(out).ok();
    }

    // ── Force-close at specified time ──────────────────────────────────────────
    if strategy.close_trades_at.is_some() {
        writeln!(out, "   // Force-close all positions at or after InpCloseHour:InpCloseMinute").ok();
        writeln!(out, "   if(dt.hour * 60 + dt.min >= InpCloseHour * 60 + InpCloseMinute)").ok();
        writeln!(out, "   {{").ok();
        writeln!(out, "      CloseAllPositions();").ok();
        writeln!(out, "      return;").ok();
        writeln!(out, "   }}").ok();
        writeln!(out).ok();
    }

    // ── Check if this EA has an open position (magic-number filtered) ──────────
    writeln!(out, "   bool hasPosition = false;").ok();
    writeln!(out, "   for(int _pi = 0; _pi < PositionsTotal(); _pi++)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      if(PositionGetSymbol(_pi) == _Symbol &&").ok();
    writeln!(out, "         (long)PositionGetInteger(POSITION_MAGIC) == (long)InpMagicNumber)").ok();
    writeln!(out, "      {{").ok();
    writeln!(out, "         hasPosition = true;").ok();
    writeln!(out, "         break;").ok();
    writeln!(out, "      }}").ok();
    writeln!(out, "   }}").ok();
    writeln!(out).ok();

    // ── Entry logic ────────────────────────────────────────────────────────────
    writeln!(out, "   if(!hasPosition)").ok();
    writeln!(out, "   {{").ok();

    // Build guard conditions for entry
    let mut guard_parts: Vec<&str> = Vec::new();
    if strategy.trading_hours.is_some()     { guard_parts.push("inHours"); }
    if strategy.max_daily_trades.is_some()  { guard_parts.push("g_dailyTradeCount < InpMaxDailyTrades"); }

    if guard_parts.is_empty() {
        // No guard — emit entries directly
        if can_long && has_long_entry {
            writeln!(out, "      if(CheckLongEntry())").ok();
            writeln!(out, "         OpenLong();").ok();
        } else if can_long {
            writeln!(out, "      // WARNING: no long entry rules defined").ok();
        }
        if can_short && has_short_entry {
            let kw = if can_long && has_long_entry { "else if" } else { "if" };
            writeln!(out, "      {}(CheckShortEntry())", kw).ok();
            writeln!(out, "         OpenShort();").ok();
        } else if can_short {
            writeln!(out, "      // WARNING: no short entry rules defined").ok();
        }
    } else {
        // Wrap entries in a guard block so if/else if chain is syntactically correct
        writeln!(out, "      if({})   // time/count filter", guard_parts.join(" && ")).ok();
        writeln!(out, "      {{").ok();
        if can_long && has_long_entry {
            writeln!(out, "         if(CheckLongEntry())").ok();
            writeln!(out, "            OpenLong();").ok();
        } else if can_long {
            writeln!(out, "         // WARNING: no long entry rules defined").ok();
        }
        if can_short && has_short_entry {
            let kw = if can_long && has_long_entry { "else if" } else { "if" };
            writeln!(out, "         {}(CheckShortEntry())", kw).ok();
            writeln!(out, "            OpenShort();").ok();
        } else if can_short {
            writeln!(out, "         // WARNING: no short entry rules defined").ok();
        }
        writeln!(out, "      }}").ok();
    }

    writeln!(out, "   }}").ok();

    // ── Exit logic ─────────────────────────────────────────────────────────────
    writeln!(out, "   else").ok();
    writeln!(out, "   {{").ok();
    // Select the position by magic number before reading its type
    writeln!(out, "      // Select our position to read POSITION_TYPE").ok();
    writeln!(out, "      for(int _pi = 0; _pi < PositionsTotal(); _pi++)").ok();
    writeln!(out, "      {{").ok();
    writeln!(out, "         if(PositionGetSymbol(_pi) == _Symbol &&").ok();
    writeln!(out, "            (long)PositionGetInteger(POSITION_MAGIC) == (long)InpMagicNumber)").ok();
    writeln!(out, "         {{").ok();
    writeln!(out, "            PositionSelectByTicket(PositionGetTicket(_pi));").ok();
    writeln!(out, "            break;").ok();
    writeln!(out, "         }}").ok();
    writeln!(out, "      }}").ok();
    writeln!(out, "      long posType = PositionGetInteger(POSITION_TYPE);").ok();

    // ── Close after N bars ─────────────────────────────────────────────────
    if strategy.close_after_bars.is_some() {
        writeln!(out, "      // Close after N bars").ok();
        writeln!(out, "      int _barsSinceEntry = iBarShift(_Symbol, PERIOD_CURRENT, (datetime)PositionGetInteger(POSITION_TIME), false);").ok();
        writeln!(out, "      if(_barsSinceEntry >= InpCloseAfterBars)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         ClosePosition();").ok();
        writeln!(out, "         return;").ok();
        writeln!(out, "      }}").ok();
        writeln!(out).ok();
    }

    let mut first_exit = true;
    if can_long && has_long_exit {
        writeln!(out, "      if(posType == POSITION_TYPE_BUY && CheckLongExit())").ok();
        writeln!(out, "         ClosePosition();").ok();
        first_exit = false;
    }
    if can_short && has_short_exit {
        let kw = if !first_exit { "else if" } else { "if" };
        writeln!(out, "      {}(posType == POSITION_TYPE_SELL && CheckShortExit())", kw).ok();
        writeln!(out, "         ClosePosition();").ok();
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
/// extra_shift=1 for "current" so that buf[1] = shift=1 = previous CLOSED bar,
/// matching the Rust engine which evaluates rules on bar[i-1].
fn mql5_rule_expr(rule: &Rule, indicators: &[UniqueIndicator]) -> String {
    let left_curr = mql5_operand_expr(&rule.left_operand,  1, indicators);
    let right_curr = mql5_operand_expr(&rule.right_operand, 1, indicators);
    match rule.comparator {
        Comparator::GreaterThan   => format!("{} > {}",  left_curr, right_curr),
        Comparator::LessThan      => format!("{} < {}",  left_curr, right_curr),
        Comparator::GreaterOrEqual => format!("{} >= {}", left_curr, right_curr),
        Comparator::LessOrEqual   => format!("{} <= {}", left_curr, right_curr),
        Comparator::Equal         => format!("{} == {}", left_curr, right_curr),
        Comparator::CrossAbove => {
            // "previous" = 2 bars back (shift=2) to match Rust bar[i-2] at eval time
            let lp = mql5_operand_expr(&rule.left_operand,  2, indicators);
            let rp = mql5_operand_expr(&rule.right_operand, 2, indicators);
            format!("({} <= {} && {} > {})", lp, rp, left_curr, right_curr)
        }
        Comparator::CrossBelow => {
            let lp = mql5_operand_expr(&rule.left_operand,  2, indicators);
            let rp = mql5_operand_expr(&rule.right_operand, 2, indicators);
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
                writeln!(out, "   if(CopyBuffer({}, {}, 0, 5, {}{}) < 5) return false;",
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
                    // PERIOD_D1 uses daily-bar indices, not intraday bar indices.
                    // extra_shift (1 or 2) must NOT be added here — it is an intraday
                    // concept and would shift into the wrong day.
                    let daily_shift = operand.offset.unwrap_or(0);
                    format!("{}{})", func, daily_shift)
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

fn mql5_open_position(out: &mut String, is_long: bool) {
    let direction  = if is_long { "Long"           } else { "Short"           };
    let order_type = if is_long { "ORDER_TYPE_BUY" } else { "ORDER_TYPE_SELL" };
    let price_sym  = if is_long { "SYMBOL_ASK"     } else { "SYMBOL_BID"      };

    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void Open{}()", direction).ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   double price = SymbolInfoDouble(_Symbol, {});", price_sym).ok();
    writeln!(out, "   double sl    = CalculateSL({}, price);", order_type).ok();
    writeln!(out, "   double lots  = CalculateLotSize(price, sl);").ok();
    writeln!(out, "   double tp    = CalculateTP({}, price, sl);", order_type).ok();
    writeln!(out).ok();
    writeln!(out, "   trade.SetExpertMagicNumber(InpMagicNumber);").ok();
    writeln!(out, "   trade.PositionOpen(_Symbol, {}, lots, price, sl, tp, \"{} Entry\");", order_type, direction).ok();
    writeln!(out, "   g_dailyTradeCount++;").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
}

fn mql5_close_position(out: &mut String, strategy: &Strategy) {
    let is_anti_mg = matches!(strategy.position_sizing.sizing_type, PositionSizingType::AntiMartingale);

    writeln!(out, "//+------------------------------------------------------------------+").ok();
    writeln!(out, "void ClosePosition()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   for(int i = PositionsTotal() - 1; i >= 0; i--)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      if(PositionGetSymbol(i) == _Symbol &&").ok();
    writeln!(out, "         (long)PositionGetInteger(POSITION_MAGIC) == (long)InpMagicNumber)").ok();
    writeln!(out, "      {{").ok();
    if is_anti_mg {
        writeln!(out, "         // AntiMartingale: track consecutive losses").ok();
        writeln!(out, "         double posPnl = PositionGetDouble(POSITION_PROFIT) + PositionGetDouble(POSITION_SWAP);").ok();
        writeln!(out, "         trade.PositionClose(PositionGetTicket(i));").ok();
        writeln!(out, "         if(posPnl < 0.0) g_consecLosses++;").ok();
        writeln!(out, "         else             g_consecLosses = 0;").ok();
    } else {
        writeln!(out, "         trade.PositionClose(PositionGetTicket(i));").ok();
    }
    writeln!(out, "         return;").ok();
    writeln!(out, "      }}").ok();
    writeln!(out, "   }}").ok();
    writeln!(out, "}}").ok();
    writeln!(out).ok();
    writeln!(out, "void CloseAllPositions()").ok();
    writeln!(out, "{{").ok();
    writeln!(out, "   for(int i = PositionsTotal() - 1; i >= 0; i--)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      if(PositionGetSymbol(i) == _Symbol &&").ok();
    writeln!(out, "         (long)PositionGetInteger(POSITION_MAGIC) == (long)InpMagicNumber)").ok();
    writeln!(out, "      {{").ok();
    if is_anti_mg {
        writeln!(out, "         double posPnl = PositionGetDouble(POSITION_PROFIT) + PositionGetDouble(POSITION_SWAP);").ok();
        writeln!(out, "         trade.PositionClose(PositionGetTicket(i));").ok();
        writeln!(out, "         if(posPnl < 0.0) g_consecLosses++;").ok();
        writeln!(out, "         else             g_consecLosses = 0;").ok();
    } else {
        writeln!(out, "         trade.PositionClose(PositionGetTicket(i));").ok();
    }
    writeln!(out, "      }}").ok();
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
            writeln!(out, "   // AntiMartingale: reduce risk after each consecutive loss").ok();
            writeln!(out, "   // scale = 1/(1+N) so 0 losses=100%, 1 loss=50%, 2 losses=33%, etc.").ok();
            writeln!(out, "   double equity      = AccountInfoDouble(ACCOUNT_EQUITY);").ok();
            writeln!(out, "   double tickValue   = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_VALUE);").ok();
            writeln!(out, "   double tickSize    = SymbolInfoDouble(_Symbol, SYMBOL_TRADE_TICK_SIZE);").ok();
            writeln!(out, "   if(sl == 0 || tickValue <= 0 || tickSize <= 0) return minLot;").ok();
            writeln!(out, "   double scaleFactor   = 1.0 / (1.0 + g_consecLosses);").ok();
            writeln!(out, "   double riskAmount    = equity * InpRiskPct / 100.0 * scaleFactor;").ok();
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
                writeln!(out, "   if(CopyBuffer(handle_{}, 0, 1, 1, atrBuf) < 1) return 0;", var).ok();
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
                writeln!(out, "   if(CopyBuffer(handle_{}, 0, 1, 1, atrBuf) < 1) return 0;", var).ok();
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
    // Position is already selected by the exit block in OnTick
    writeln!(out, "   double currentSL  = PositionGetDouble(POSITION_SL);").ok();
    writeln!(out, "   double entryPrice = PositionGetDouble(POSITION_PRICE_OPEN);").ok();
    writeln!(out, "   long   posType    = PositionGetInteger(POSITION_TYPE);").ok();
    writeln!(out, "   ulong  ticket     = PositionGetInteger(POSITION_TICKET);").ok();
    writeln!(out).ok();

    match ts.ts_type {
        TrailingStopType::ATR => {
            let var = format!("atr_{}", ts.atr_period.unwrap_or(14));
            writeln!(out, "   double atrBuf[];").ok();
            writeln!(out, "   ArraySetAsSeries(atrBuf, true);").ok();
            writeln!(out, "   if(CopyBuffer(handle_{}, 0, 1, 1, atrBuf) < 1) return;", var).ok();
            writeln!(out, "   double trailDist = atrBuf[0] * InpTSAtrMult;").ok();
        }
        TrailingStopType::RiskReward => {
            writeln!(out, "   // R:R trailing: distance = initial_SL_distance * RR_multiplier").ok();
            writeln!(out, "   double initialSlDist = MathAbs(entryPrice - currentSL);").ok();
            writeln!(out, "   if(initialSlDist <= 0) return; // no SL set yet").ok();
            writeln!(out, "   double trailDist = initialSlDist * InpTSRR;").ok();
        }
    }

    writeln!(out).ok();
    writeln!(out, "   if(posType == POSITION_TYPE_BUY)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      double newSL = SymbolInfoDouble(_Symbol, SYMBOL_BID) - trailDist;").ok();
    writeln!(out, "      newSL = NormalizeDouble(newSL, _Digits);").ok();
    writeln!(out, "      // Only tighten — never move SL further from current price").ok();
    writeln!(out, "      if(newSL > currentSL)").ok();
    writeln!(out, "         trade.PositionModifyByTicket(ticket, newSL, PositionGetDouble(POSITION_TP));").ok();
    writeln!(out, "   }}").ok();
    writeln!(out, "   else if(posType == POSITION_TYPE_SELL)").ok();
    writeln!(out, "   {{").ok();
    writeln!(out, "      double newSL = SymbolInfoDouble(_Symbol, SYMBOL_ASK) + trailDist;").ok();
    writeln!(out, "      newSL = NormalizeDouble(newSL, _Digits);").ok();
    writeln!(out, "      // Only tighten — never move SL further from current price").ok();
    writeln!(out, "      if(currentSL == 0 || newSL < currentSL)").ok();
    writeln!(out, "         trade.PositionModifyByTicket(ticket, newSL, PositionGetDouble(POSITION_TP));").ok();
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
            IndicatorType::AwesomeOscillator |
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
            // ── New SQX indicators ──
            IndicatorType::AvgVolume => {
                writeln!(out, "{0} = ta.sma(volume, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::BBWidthRatio => {
                writeln!(out, "// Bollinger Bands Width Ratio").ok();
                writeln!(out, "{0}_basis = ta.sma(close, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_sd = ta.stdev(close, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0} = {0}_basis != 0 ? (2 * i_{0}_stddev * {0}_sd) / {0}_basis : 0", ind.var_name).ok();
            }
            IndicatorType::EfficiencyRatio => {
                writeln!(out, "// Kaufman Efficiency Ratio").ok();
                writeln!(out, "{0}_dir = math.abs(close - close[i_{0}_period])", ind.var_name).ok();
                writeln!(out, "{0}_noise = ta.sum(math.abs(close - close[1]), i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0} = {0}_noise != 0 ? {0}_dir / {0}_noise : 0", ind.var_name).ok();
            }
            IndicatorType::HighestIndex => {
                writeln!(out, "{0} = ta.highestbars(high, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::KAMA => {
                writeln!(out, "// Kaufman Adaptive MA (approximated as EMA — use SqKAMA in MT5 for exact calc)").ok();
                writeln!(out, "{0} = ta.ema(close, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::LowestIndex => {
                writeln!(out, "{0} = ta.lowestbars(low, i_{0}_period)", ind.var_name).ok();
            }
            IndicatorType::QQE => {
                writeln!(out, "// QQE — approximated as smoothed RSI (use SqQQE in MT5 for exact calc)").ok();
                writeln!(out, "{0}_rsi = ta.rsi(close, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0} = ta.ema({0}_rsi, i_{0}_sf)", ind.var_name).ok();
            }
            IndicatorType::SchaffTrendCycle => {
                writeln!(out, "// Schaff Trend Cycle (use SqSchaffTrendCycle in MT5 for exact calc)").ok();
                writeln!(out, "{0}_macd = ta.ema(close, i_{0}_fast) - ta.ema(close, i_{0}_slow)", ind.var_name).ok();
                writeln!(out, "{0}_ll = ta.lowest({0}_macd, i_{0}_period)", ind.var_name).ok();
                writeln!(out, "{0}_hh = ta.highest({0}_macd, i_{0}_period) - {0}_ll", ind.var_name).ok();
                writeln!(out, "{0} = {0}_hh != 0 ? 100 * ({0}_macd - {0}_ll) / {0}_hh : na", ind.var_name).ok();
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
        // ── Non-SQX indicators: generate BT_* custom files ──
        IndicatorType::SMA => ("BT_SMA.mq5".into(), gen_mql5_sma()),
        IndicatorType::EMA => ("BT_EMA.mq5".into(), gen_mql5_ema()),
        IndicatorType::RSI => ("BT_RSI.mq5".into(), gen_mql5_rsi()),
        IndicatorType::MACD => ("BT_MACD.mq5".into(), gen_mql5_macd()),
        IndicatorType::BollingerBands => ("BT_BollingerBands.mq5".into(), gen_mql5_bollinger()),
        IndicatorType::BarRange => ("BT_BarRange.mq5".into(), gen_mql5_bar_range()),
        IndicatorType::BiggestRange => ("BT_BiggestRange.mq5".into(), gen_mql5_biggest_range()),
        IndicatorType::SmallestRange => ("BT_SmallestRange.mq5".into(), gen_mql5_smallest_range()),
        IndicatorType::Fibonacci => ("BT_Fibonacci.mq5".into(), gen_mql5_fibonacci()),
        IndicatorType::Pivots    => ("BT_Pivots.mq5".into(),    gen_mql5_pivots()),
        // ── SQX indicators: generate self-contained BT_* files ──
        IndicatorType::ATR              => ("BT_ATR.mq5".into(),              gen_mql5_atr()),
        IndicatorType::ADX              => ("BT_ADX.mq5".into(),              gen_mql5_adx()),
        IndicatorType::Stochastic       => ("BT_Stochastic.mq5".into(),       gen_mql5_stochastic()),
        IndicatorType::CCI              => ("BT_CCI.mq5".into(),              gen_mql5_cci()),
        IndicatorType::WilliamsR        => ("BT_WilliamsR.mq5".into(),        gen_mql5_wpr()),
        IndicatorType::ParabolicSAR     => ("BT_ParabolicSAR.mq5".into(),     gen_mql5_parabolic_sar()),
        IndicatorType::ROC              => ("BT_ROC.mq5".into(),              gen_mql5_roc()),
        IndicatorType::Ichimoku         => ("BT_Ichimoku.mq5".into(),         gen_mql5_ichimoku()),
        IndicatorType::KeltnerChannel   => ("BT_KeltnerChannel.mq5".into(),   gen_mql5_keltner_channel()),
        IndicatorType::SuperTrend       => ("BT_SuperTrend.mq5".into(),       gen_mql5_supertrend()),
        IndicatorType::LaguerreRSI      => ("BT_LaguerreRSI.mq5".into(),      gen_mql5_laguerre_rsi()),
        IndicatorType::BearsPower       => ("BT_BearsPower.mq5".into(),       gen_mql5_bears_power()),
        IndicatorType::BullsPower       => ("BT_BullsPower.mq5".into(),       gen_mql5_bulls_power()),
        IndicatorType::TrueRange        => ("BT_TrueRange.mq5".into(),        gen_mql5_true_range()),
        IndicatorType::LinearRegression => ("BT_LinearRegression.mq5".into(), gen_mql5_linreg()),
        IndicatorType::Fractal          => ("BT_Fractal.mq5".into(),          gen_mql5_fractal()),
        IndicatorType::HeikenAshi       => ("BT_HeikenAshi.mq5".into(),       gen_mql5_heiken_ashi()),
        IndicatorType::GannHiLo         => ("BT_GannHiLo.mq5".into(),         gen_mql5_gann_hi_lo()),
        IndicatorType::HullMA           => ("BT_HullMA.mq5".into(),           gen_mql5_hull_ma()),
        IndicatorType::UlcerIndex       => ("BT_UlcerIndex.mq5".into(),       gen_mql5_ulcer_index()),
        IndicatorType::Vortex           => ("BT_Vortex.mq5".into(),           gen_mql5_vortex()),
        IndicatorType::Aroon            => ("BT_Aroon.mq5".into(),            gen_mql5_aroon()),
        IndicatorType::HighestInRange   => ("BT_HighestInRange.mq5".into(),   gen_mql5_highest_in_range()),
        IndicatorType::LowestInRange    => ("BT_LowestInRange.mq5".into(),    gen_mql5_lowest_in_range()),
        IndicatorType::Reflex           => ("BT_Reflex.mq5".into(),           gen_mql5_reflex()),
        IndicatorType::AvgVolume        => ("BT_AvgVolume.mq5".into(),        gen_mql5_avg_volume()),
        IndicatorType::BBWidthRatio     => ("BT_BBWidthRatio.mq5".into(),     gen_mql5_bb_width_ratio()),
        IndicatorType::EfficiencyRatio  => ("BT_EfficiencyRatio.mq5".into(),  gen_mql5_efficiency_ratio()),
        IndicatorType::HighestIndex     => ("BT_HighestIndex.mq5".into(),     gen_mql5_highest_index()),
        IndicatorType::KAMA             => ("BT_KAMA.mq5".into(),             gen_mql5_kama()),
        IndicatorType::LowestIndex      => ("BT_LowestIndex.mq5".into(),      gen_mql5_lowest_index()),
        IndicatorType::QQE              => ("BT_QQE.mq5".into(),              gen_mql5_qqe()),
        IndicatorType::SchaffTrendCycle => ("BT_SchaffTrendCycle.mq5".into(), gen_mql5_schaff_trend_cycle()),
        // Native handles or no file needed
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


// ── BT_BarRange ──

fn gen_mql5_bar_range() -> String {
    let mut out = mql5_indicator_header("BT_BarRange");
    out.push_str(r#"#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_label1  "BarRange"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrDodgerBlue
double ExtBuffer[];
int OnInit() { SetIndexBuffer(0, ExtBuffer, INDICATOR_DATA); IndicatorSetString(INDICATOR_SHORTNAME, "BT_BarRange"); return INIT_SUCCEEDED; }
int OnCalculate(const int rates_total, const int prev_calculated, const datetime &time[], const double &open[], const double &high[], const double &low[], const double &close[], const long &tick_volume[], const long &volume[], const int &spread[]) {
   for(int i = (prev_calculated > 0 ? prev_calculated - 1 : 0); i < rates_total; i++)
      ExtBuffer[i] = high[i] - low[i];
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
#property indicator_label1  "BiggestRange"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrRed
input int InpPeriod = 14;
double ExtBuffer[];
int OnInit() { SetIndexBuffer(0, ExtBuffer, INDICATOR_DATA); IndicatorSetString(INDICATOR_SHORTNAME, "BT_BiggestRange"); return INIT_SUCCEEDED; }
int OnCalculate(const int rates_total, const int prev_calculated, const datetime &time[], const double &open[], const double &high[], const double &low[], const double &close[], const long &tick_volume[], const long &volume[], const int &spread[]) {
   int start = (prev_calculated > InpPeriod ? prev_calculated - 1 : InpPeriod - 1);
   for(int i = start; i < rates_total; i++) {
      double maxRange = 0;
      for(int j = 0; j < InpPeriod; j++) maxRange = MathMax(maxRange, high[i-j] - low[i-j]);
      ExtBuffer[i] = maxRange;
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
#property indicator_label1  "SmallestRange"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrGreen
input int InpPeriod = 14;
double ExtBuffer[];
int OnInit() { SetIndexBuffer(0, ExtBuffer, INDICATOR_DATA); IndicatorSetString(INDICATOR_SHORTNAME, "BT_SmallestRange"); return INIT_SUCCEEDED; }
int OnCalculate(const int rates_total, const int prev_calculated, const datetime &time[], const double &open[], const double &high[], const double &low[], const double &close[], const long &tick_volume[], const long &volume[], const int &spread[]) {
   int start = (prev_calculated > InpPeriod ? prev_calculated - 1 : InpPeriod - 1);
   for(int i = start; i < rates_total; i++) {
      double minRange = DBL_MAX;
      for(int j = 0; j < InpPeriod; j++) minRange = MathMin(minRange, high[i-j] - low[i-j]);
      ExtBuffer[i] = minRange;
   }
   return rates_total;
}
"#);
    out
}

// ── BT_Fibonacci ──

fn gen_mql5_fibonacci() -> String {
r#"//+------------------------------------------------------------------+
//|                                                   BT_Fibonacci.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright "Copyright © 2017, StrategyQuant s.r.o."
#property link      "http://www.strategyquant.com"
#property version   "1.00"
#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "Fibo level"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Red

//---- input parameters
//+------------------------------------------------------------------+
//|  FiboRange options:                                              |
//|  1  - High-Low previous day                                      |
//|  2  - High-low previous week                                     |
//|  3  - High-low previous month                                    |
//|  4  - High-Low of last X days                                    |
//|  5  - Open-Close previous day                                    |
//|  6  - Open-Close previous week                                   |
//|  7  - Open-Close previous month                                  |
//|  8  - Open-Close of last X days                                  |
//|  9  - Highest-Lowest for last X bars back                        |
//|  10 - Open-Close for last X bars back                            |
//|                                                                  |
//|  Custom Fibo Level (to be used by SQ):                           |
//|  Set FiboLevel to -9999999 and specify CustomFiboLevel           |
//+------------------------------------------------------------------+

input int FiboRange = 1;         //Fibo range mode [1-10]
input int X;                     //Custom days/bars count
input double FiboLevel = 61.8;
input double CustomFiboLevel;
input datetime StartDate = 0;    //Start point for calculations

//---- buffers
double buffer[];

//---- variables
uint tfEndTime = 0;
int barsUsed = -1;
double prevTFOpen = 0, prevTFHigh = 0, prevTFLow = 0, prevTFClose = 0;
double fiboLevel = 0;
int fiboRangeUsed = 0;
datetime lastBarTime = 0;
bool startDateUsed = false;

//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {

   fiboLevel = FiboLevel == -9999999 ? CustomFiboLevel : FiboLevel;
   fiboRangeUsed = FiboRange >= 1 && FiboRange <= 7 ? FiboRange : 1;
//--- indicator buffers mapping

   ArraySetAsSeries(buffer, true);
   SetIndexBuffer(0, buffer);

//---
   string short_name = "Fibo(" + IntegerToString(fiboRangeUsed)+ ", " + DoubleToString(fiboLevel) + ")";
   IndicatorSetString(INDICATOR_SHORTNAME, short_name);

   return(INIT_SUCCEEDED);
  }

//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//---

   ArraySetAsSeries(time, true);
   ArraySetAsSeries(open, true);
   ArraySetAsSeries(high, true);
   ArraySetAsSeries(low, true);
   ArraySetAsSeries(close, true);

   int limit = rates_total - prev_calculated;

   for(int i=limit - 1; i>=0; i--){
      if(isNewTFStart(time[i])){
         double upperValue = 0, lowerValue = 0;

         switch(fiboRangeUsed){
            case 1:
            case 2:
            case 3:
            case 4:
            case 9:
               upperValue = prevTFHigh;
               lowerValue = prevTFLow;
               break;
            case 5:
            case 6:
            case 7:
            case 8:
            case 10:
               upperValue = MathMax(prevTFOpen, prevTFClose);
               lowerValue = MathMin(prevTFOpen, prevTFClose);
               break;
         }

         double percentStep = (upperValue - lowerValue) / 100;
         double fiboPct = FiboLevel == -9999999 ? CustomFiboLevel : FiboLevel;
         double delta = fiboPct * percentStep;

         bool bullish = prevTFClose > prevTFOpen;

         fiboLevel = bullish ? (upperValue - delta) : (lowerValue + delta);

         prevTFOpen = open[i];
         prevTFHigh = high[i];
         prevTFLow = low[i];
         prevTFClose = close[i];

         barsUsed = i == 0 ? 0 : 1;
      }
      else {
         if(i != 0){
            prevTFHigh = MathMax(prevTFHigh, high[i]);
            prevTFLow = MathMin(prevTFLow, low[i]);
            prevTFClose = close[i];

            barsUsed++;

         }
      }

      buffer[i] = fiboLevel;
   }

//--- return value of prev_calculated for next call
   return(rates_total - 1);
  }
//+------------------------------------------------------------------+

bool isNewTFStart(datetime time){
   if(StartDate != 0 && !startDateUsed && StartDate <= time) {
      setEndTime(time);
      startDateUsed = true;
      return true;
   }

   switch(fiboRangeUsed){
      case 9:
      case 10:
         if(barsUsed == -1 || barsUsed == X){
            return true;
         }
         else return false;
      case 1:
      case 2:
      case 3:
      case 4:
      case 5:
      case 6:
      case 7:
      case 8:
         if(tfEndTime == 0 || tfEndTime <= (uint) time){
            setEndTime(time);
            return true;
         }
         else return false;
      default:
         Alert("Invalid FiboRange used: " + IntegerToString(fiboRangeUsed));
         return false;
   }
}

void setEndTime(datetime time){
   uint curDayStart = (uint) time;
   curDayStart = curDayStart - (curDayStart % (24 * 3600));

   MqlDateTime startDateTime;
   TimeToStruct(time, startDateTime);

   switch(fiboRangeUsed){
      case 1:
      case 5:
         tfEndTime = curDayStart + (24 * 3600);
         break;
      case 2:
      case 6:
         curDayStart -= getWeekDayIndex(startDateTime) * 24 * 3600;
         tfEndTime = curDayStart + (7 * 24 * 3600);
         break;
      case 3:
      case 7:
         curDayStart = curDayStart - ((startDateTime.day - 1) * 24 * 3600);
         tfEndTime = curDayStart + (getMonthDaysCount(curDayStart) * 24 * 3600);
         break;
      case 4:
      case 8:
         tfEndTime = curDayStart + (X * 24 * 3600);
         break;
   }
}

int getMonthDaysCount(uint time){
   MqlDateTime timeStruct;
   TimeToStruct(time, timeStruct);
   int month = timeStruct.mon;
   int days = 0;

   while(timeStruct.day != 1 || timeStruct.mon == month){
      time += 24 * 3600;
      TimeToStruct(time, timeStruct);
      days++;
   }

   return days;
}

int getWeekDayIndex(MqlDateTime &dateTime){
   int dayOfWeek = dateTime.day_of_week - 1;
   return dayOfWeek < 0 ? 6 : dayOfWeek;
}
"#.to_string()
}

// ── BT_Pivots ──

fn gen_mql5_pivots() -> String {
r##"//+------------------------------------------------------------------+
//|                                                      BT_Pivots.mq5 |
//|                           Copyright © 2019, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright "Copyright © 2019, StrategyQuant s.r.o."
#property link      "http://www.strategyquant.com"
#property version   "1.00"
#property indicator_chart_window
#property indicator_buffers 7
#property indicator_plots 7

#property indicator_label1  "PP"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Green

#property indicator_label2  "R1"
#property indicator_type2  DRAW_LINE
#property indicator_color2 Blue

#property indicator_label3  "R2"
#property indicator_type3  DRAW_LINE
#property indicator_color3 Blue

#property indicator_label4  "R3"
#property indicator_type4  DRAW_LINE
#property indicator_color4 Blue

#property indicator_label5  "S1"
#property indicator_type5  DRAW_LINE
#property indicator_color5 Red

#property indicator_label6  "S2"
#property indicator_type6  DRAW_LINE
#property indicator_color6 Red

#property indicator_label7  "S3"
#property indicator_type7  DRAW_LINE
#property indicator_color7 Red

//---- input parameters
input int       StartHour=8;
input int       StartMinute=20;
input int       DaysToPlot=0;
input color     SupportLabelColor=DodgerBlue;
input color     ResistanceLabelColor=OrangeRed;
input color     PivotLabelColor=Green;
input int       fontsize=8;
input int       LabelShift = 0;

//---- buffers
double R3Buffer[];
double R2Buffer[];
double R1Buffer[];
double PBuffer[];
double S1Buffer[];
double S2Buffer[];
double S3Buffer[];


string Pivot="Pivot",Sup1="S 1", Res1="R 1";
string Sup2="S 2", Res2="R 2", Sup3="S 3", Res3="R 3";

datetime LabelShiftTime;
int PeriodMinutes;
int StartMinutesIntoDay, CloseMinutesIntoDay;

int PreviousClosingBar = 1;
datetime PreviousClosingTime = 0;

double PreviousHigh = 0;
double PreviousLow = 0;
double PreviousClose = 0;

double P = 0, S1 = 0, R1 = 0, S2 = 0, R2 = 0, S3 = 0, R3 = 0;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- indicator buffers mapping

   SetIndexBuffer(0,PBuffer);
   SetIndexBuffer(1,R1Buffer);
   SetIndexBuffer(2,R2Buffer);
   SetIndexBuffer(3,R3Buffer);
   SetIndexBuffer(4,S1Buffer);
   SetIndexBuffer(5,S2Buffer);
   SetIndexBuffer(6,S3Buffer);

   ArraySetAsSeries(PBuffer, true);
   ArraySetAsSeries(S1Buffer, true);
   ArraySetAsSeries(R1Buffer, true);
   ArraySetAsSeries(S2Buffer, true);
   ArraySetAsSeries(R2Buffer, true);
   ArraySetAsSeries(S3Buffer, true);
   ArraySetAsSeries(R3Buffer, true);

   IndicatorSetString(INDICATOR_SHORTNAME,"Pivots");

   PeriodMinutes = PeriodSeconds(Period()) / 60;
   StartMinutesIntoDay = correctStartMinutes((StartHour * 60) + StartMinute);
   CloseMinutesIntoDay = StartMinutesIntoDay - PeriodMinutes;
//---
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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

//---- indicator calculation
   int min = 1440 / PeriodMinutes;
   int start = prev_calculated;

   if(start < 0) return -1;

   for(int i=0; i<rates_total - start; i++){
       R3Buffer[i]=0;
       R2Buffer[i]=0;
       R1Buffer[i]=0;
       PBuffer[i]=0;
       S1Buffer[i]=0;
       S2Buffer[i]=0;
       S3Buffer[i]=0;
   }

   if(rates_total < min) return(0);

   int limit=rates_total - ((start + 1) > min ? start : min);

   ArraySetAsSeries(time, true);
   ArraySetAsSeries(open, true);
   ArraySetAsSeries(high, true);
   ArraySetAsSeries(low, true);
   ArraySetAsSeries(close, true);

   if (CloseMinutesIntoDay < 0){
      CloseMinutesIntoDay = CloseMinutesIntoDay + 1440;
   }

   int BarsInDay = 1440 / PeriodMinutes;

   for(int i=limit; i>=0; i--){
      if ((i < ((DaysToPlot + 1) * BarsInDay)) || DaysToPlot == 0){

         PreviousClosingBar = FindLastTimeMatchFast(CloseMinutesIntoDay, i + 1, time, PreviousClosingBar, true);

         if(PreviousClosingTime != time[PreviousClosingBar]) {
            PreviousClosingTime = time[PreviousClosingBar];

            int PreviousOpeningBar = FindLastTimeMatchFast(StartMinutesIntoDay, PreviousClosingBar + 1, time, 1000000, false);

            PreviousHigh = high[PreviousClosingBar];
            PreviousLow = low [PreviousClosingBar];
            PreviousClose = close[PreviousClosingBar];

            for (int SearchHighLow = PreviousClosingBar; SearchHighLow < PreviousOpeningBar + 1; SearchHighLow++){
               if(SearchHighLow == ArraySize(time)) break;

               if (high[SearchHighLow] > PreviousHigh) PreviousHigh = high[SearchHighLow];
               if (low[SearchHighLow] < PreviousLow) PreviousLow = low[SearchHighLow];
            }
         }

         P =  (PreviousHigh + PreviousLow + PreviousClose) / 3;
         R1 = (2 * P) - PreviousLow;
         S1 = (2 * P) - PreviousHigh;
         R2 =  P + (PreviousHigh - PreviousLow);
         S2 =  P - (PreviousHigh - PreviousLow);
         R3 =  P + 2 * (PreviousHigh - PreviousLow);
         S3 =  P - 2 * (PreviousHigh - PreviousLow);

         LabelShiftTime = time[LabelShift];

         if (i == 0){

            ObjectCreate(ChartID(), "Pivot", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Pivot", OBJPROP_TEXT, "                           Pivot " +DoubleToString(P,4));
            ObjectCreate(ChartID(),"Sup1", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Sup1", OBJPROP_TEXT, "                    S1 " +DoubleToString(S1,4));
            ObjectCreate(ChartID(),"Res1", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Res1", OBJPROP_TEXT, "                    R1 " +DoubleToString(R1,4));
            ObjectCreate(ChartID(),"Sup2", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Sup2", OBJPROP_TEXT, "                    S2 " +DoubleToString(S2,4));
            ObjectCreate(ChartID(),"Res2", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Res2", OBJPROP_TEXT, "                    R2 " +DoubleToString(R2,4));
            ObjectCreate(ChartID(),"Sup3", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Sup3", OBJPROP_TEXT, "                    S3 " +DoubleToString(S3,4));
            ObjectCreate(ChartID(),"Res3", OBJ_TEXT, 0, LabelShiftTime, 0);
            ObjectSetString(ChartID(), "Res3", OBJPROP_TEXT, "                    R3 " +DoubleToString(R3,4));

            ObjectMove(ChartID(),"Res3", 0, LabelShiftTime,R3);
            ObjectMove(ChartID(),"Res2", 0, LabelShiftTime,R2);
            ObjectMove(ChartID(),"Res1", 0, LabelShiftTime,R1);
            ObjectMove(ChartID(),"Pivot", 0, LabelShiftTime,P);
            ObjectMove(ChartID(),"Sup1", 0, LabelShiftTime,S1);
            ObjectMove(ChartID(),"Sup2", 0, LabelShiftTime,S2);
            ObjectMove(ChartID(),"Sup3", 0, LabelShiftTime,S3);
         }
      }

      R3Buffer[i]=R3;
      R2Buffer[i]=R2;
      R1Buffer[i]=R1;
      PBuffer[i]=P;
      S1Buffer[i]=S1;
      S2Buffer[i]=S2;
      S3Buffer[i]=S3;
   }
//--- return value of prev_calculated for next call
   return(rates_total);
//+------------------------------------------------------------------+
}


int FindLastTimeMatchFast(int TimeToLookFor, int StartingBar, const datetime &time[], int prevBarFound, bool isClosingBar){
   int HowManyBarsBack = MathMin(ArraySize(time) - 1, 1440 / PeriodMinutes * 3);

   if(checkBarIsWhatWeLookFor(TimeToLookFor, StartingBar, time, isClosingBar)) {
      return StartingBar;
   }
   else if(prevBarFound < HowManyBarsBack && checkBarIsWhatWeLookFor(TimeToLookFor, prevBarFound, time, isClosingBar)) {
      return prevBarFound;
   }
   else if(prevBarFound < HowManyBarsBack && checkBarIsWhatWeLookFor(TimeToLookFor, prevBarFound + 1, time, isClosingBar)) {
      return prevBarFound + 1;
   }
   else {
      for(int a=StartingBar + 1; a<HowManyBarsBack; a++) {
         if(checkBarIsWhatWeLookFor(TimeToLookFor, a, time, isClosingBar)) {
            return a;
         }
      }

      return HowManyBarsBack + 1;
   }
}

bool checkBarIsWhatWeLookFor(int TimeToLookFor, int bar, const datetime &time[], bool isClosingBar){
   if(bar >= ArraySize(time) - 1) return false;

   int PreviousBarsTime = (TimeHour(time[bar - 1]) * 60) + TimeMinute(time[bar - 1]);
   int CurrentBarsTime = (TimeHour(time[bar]) * 60) + TimeMinute(time[bar]);
   int NextBarsTime = (TimeHour(time[bar + 1]) * 60) + TimeMinute(time[bar + 1]);

   if(CurrentBarsTime == TimeToLookFor) return true;

   int PreviousBarDay = TimeDayOfYear(time[bar - 1]);
   int CurrentBarDay = TimeDayOfYear(time[bar]);
   int NextBarDay = TimeDayOfYear(time[bar + 1]);

   if(NextBarDay != CurrentBarDay) {
      NextBarsTime = NextBarsTime - 1440;
   }

   if(PreviousBarDay != CurrentBarDay) {
      if(PreviousBarsTime > TimeToLookFor && CurrentBarsTime > TimeToLookFor) {
         return true;
      }
      PreviousBarsTime = PreviousBarsTime + 1440;
   }

   if(PreviousBarsTime > TimeToLookFor && NextBarsTime < TimeToLookFor) {
      return isClosingBar ? CurrentBarsTime < TimeToLookFor : true;
   }

   return false;
}

int TimeMinute(datetime date){
   MqlDateTime tm;
   TimeToStruct(date,tm);
   return(tm.min);
}

int TimeHour(datetime date){
   MqlDateTime tm;
   TimeToStruct(date,tm);
   return(tm.hour);
}

int TimeDayOfYear(datetime date){
   MqlDateTime tm;
   TimeToStruct(date,tm);
   return(tm.day_of_year);
}

int correctStartMinutes(int minutes){
   int temp = minutes;
   while(temp % PeriodMinutes != 0){
      temp++;
   }
   return temp >= 1440 ? temp-1440 : temp;
}
"##.to_string()
}

// ── BT_ATR ──

fn gen_mql5_atr() -> String {
r#"//+------------------------------------------------------------------+
//|                                                        BT_ATR.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Average True Range"
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   1
#property indicator_type1   DRAW_LINE
#property indicator_color1  DodgerBlue
#property indicator_label1  "ATR"
//--- input parameters
input int InpAtrPeriod=14;  // ATR period
//--- indicator buffers
double    ExtATRBuffer[];
double    ExtTRBuffer[];
//--- global variable
int       ExtPeriodATR;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- check for input value
   if(InpAtrPeriod<=0)
     {
      ExtPeriodATR=14;
      printf("Incorrect input parameter InpAtrPeriod = %d. Indicator will use value %d for calculations.",InpAtrPeriod,ExtPeriodATR);
     }
   else ExtPeriodATR=InpAtrPeriod;
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtATRBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtTRBuffer,INDICATOR_CALCULATIONS);
//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,InpAtrPeriod);
//--- name for DataWindow and indicator subwindow label
   string short_name="ATR("+string(ExtPeriodATR)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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
   int i,limit;

   if(prev_calculated == 0){
      ExtATRBuffer[0] = high[0] - low[0];
      limit=1;
   }
   else limit=prev_calculated-1;
//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++){
      double trueRange = high[i] - low[i];
      double prevClose = close[i-1];
      trueRange = MathMax(MathAbs(low[i] - prevClose), MathMax(trueRange, MathAbs(high[i] - prevClose)));
      double multiplier = MathIsValidNumber(ExtATRBuffer[i-1]) ? ExtATRBuffer[i-1] : 0;
      ExtATRBuffer[i] = (((MathMin(i + 1, ExtPeriodATR) - 1 ) * multiplier) + trueRange) / MathMin(i + 1, ExtPeriodATR);
   }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_ADX ──

fn gen_mql5_adx() -> String {
r#"//+------------------------------------------------------------------+
//|                                                        BT_ADX.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Average Directional Movement Index"
#include <MovingAverages.mqh>

#property indicator_separate_window
#property indicator_buffers 6
#property indicator_plots   3
#property indicator_type1   DRAW_LINE
#property indicator_color1  LightSeaGreen
#property indicator_style1  STYLE_SOLID
#property indicator_width1  1
#property indicator_type2   DRAW_LINE
#property indicator_color2  YellowGreen
#property indicator_style2  STYLE_DOT
#property indicator_width2  1
#property indicator_type3   DRAW_LINE
#property indicator_color3  Wheat
#property indicator_style3  STYLE_DOT
#property indicator_width3  1
#property indicator_label1  "ADX"
#property indicator_label2  "+DI"
#property indicator_label3  "-DI"
//--- input parameters
input int InpPeriodADX=14; // Period
//---- buffers
double    ExtADXBuffer[];
double    ExtPDIBuffer[];
double    ExtNDIBuffer[];
double    ExtSumDmPlusBuffer[];
double    ExtSumDmMinusBuffer[];
double    ExtSumTrBuffer[];
//--- global variables
int       ExtADXPeriod;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- check for input parameters
   if(InpPeriodADX<=0)
     {
      ExtADXPeriod=14;
      printf("Incorrect value for input variable Period_ADX=%d. Indicator will use value=%d for calculations.",InpPeriodADX,ExtADXPeriod);
     }
   else ExtADXPeriod=InpPeriodADX;
//---- indicator buffers
   SetIndexBuffer(0,ExtADXBuffer);
   SetIndexBuffer(1,ExtPDIBuffer);
   SetIndexBuffer(2,ExtNDIBuffer);
   SetIndexBuffer(3,ExtSumDmPlusBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(4,ExtSumDmMinusBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(5,ExtSumTrBuffer,INDICATOR_CALCULATIONS);
//--- indicator digits
   IndicatorSetInteger(INDICATOR_DIGITS,2);
//--- set draw begin
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ExtADXPeriod<<1);
   PlotIndexSetInteger(1,PLOT_DRAW_BEGIN,ExtADXPeriod);
   PlotIndexSetInteger(2,PLOT_DRAW_BEGIN,ExtADXPeriod);
//--- indicator short name
   string short_name="ADX("+string(ExtADXPeriod)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//--- change 1-st index label
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//---- end of initialization function
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- detect start position
   int start;
   if(prev_calculated>1) start=prev_calculated-1;
   else
     {
      start=1;

      ExtSumDmPlusBuffer[0] = 0.0;
      ExtSumDmMinusBuffer[0] = 0.0;
      ExtSumTrBuffer[0] = high[0] - low[0];
      ExtPDIBuffer[0] = 0.0;
      ExtNDIBuffer[0] = 0.0;
      ExtADXBuffer[0] = 0.0;
     }
//--- main cycle
   for(int i=start;i<rates_total && !IsStopped();i++){
      double trueRange = high[i] - low[i];

      double High = high[i];
      double Low = low[i];

      double prevHigh = high[i-1];
      double prevLow = low[i-1];
      double prevClose = close[i-1];

      double deltaHH = NormalizeDouble(High - prevHigh, 8);
      double deltaLL = NormalizeDouble(prevLow - Low, 8);
      double deltaHC = NormalizeDouble(High - prevClose, 8);
      double deltaLC = NormalizeDouble(Low - prevClose, 8);

      double tr = MathMax(MathAbs(deltaLC), MathMax(trueRange, MathAbs(deltaHC)));
      double dmPlus = deltaHH > deltaLL ? MathMax(deltaHH, 0) : 0;
      double dmMinus = deltaLL > deltaHH ? MathMax(deltaLL, 0) : 0;

      if (i < ExtADXPeriod){
         ExtSumTrBuffer[i] = NormalizeDouble(ExtSumTrBuffer[i-1] + tr, 8);
         ExtSumDmPlusBuffer[i] = ExtSumDmPlusBuffer[i-1] + dmPlus;
         ExtSumDmMinusBuffer[i] = ExtSumDmMinusBuffer[i-1] + dmMinus;
      }
      else {
         ExtSumTrBuffer[i] = NormalizeDouble(ExtSumTrBuffer[i-1] - ExtSumTrBuffer[i-1] / ExtADXPeriod + tr, 8);
         ExtSumDmPlusBuffer[i] = ExtSumDmPlusBuffer[i-1] - ExtSumDmPlusBuffer[i-1] / ExtADXPeriod + dmPlus;
         ExtSumDmMinusBuffer[i] = ExtSumDmMinusBuffer[i-1] - ExtSumDmMinusBuffer[i-1] / ExtADXPeriod + dmMinus;
      }

      ExtPDIBuffer[i] = 100 * (ExtSumTrBuffer[i] == 0 ? 0 : ExtSumDmPlusBuffer[i] / ExtSumTrBuffer[i]);
      ExtNDIBuffer[i] = 100 * (ExtSumTrBuffer[i] == 0 ? 0 : ExtSumDmMinusBuffer[i] / ExtSumTrBuffer[i]);

      double diff = MathAbs(ExtPDIBuffer[i] - ExtNDIBuffer[i]);
      double sum = NormalizeDouble(ExtPDIBuffer[i] + ExtNDIBuffer[i], 8);

      ExtADXBuffer[i] = sum == 0 ? 50 : ((ExtADXPeriod - 1) * ExtADXBuffer[i-1] + 100 * diff / sum) / ExtADXPeriod;

   }
//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_Stochastic ──

fn gen_mql5_stochastic() -> String {
r#"//+------------------------------------------------------------------+
//|                                                  BT_Stochastic.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#include <MovingAverages.mqh>
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 3
#property indicator_plots   2
#property indicator_type1   DRAW_LINE
#property indicator_type2   DRAW_LINE
#property indicator_color1  LightSeaGreen
#property indicator_color2  Red
#property indicator_style2  STYLE_DOT
//--- input parameters
input int InpKPeriod=5;  // K period
input int InpDPeriod=3;  // D period
input int InpSlowing=3;  // Slowing
input ENUM_MA_METHOD       InpAppliedMA=MODE_SMA;                       // Applied MA method for signal line
input ENUM_STO_PRICE       InpAppliedPrice=STO_LOWHIGH;                 // Applied price

ENUM_MA_METHOD AppliedMA;
ENUM_STO_PRICE AppliedPrice;

//--- indicator buffers
double ExtMainBuffer[];
double ExtSignalBuffer[];

double _k[];

//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   ArraySetAsSeries(ExtMainBuffer, false);
   ArraySetAsSeries(ExtSignalBuffer, false);
   ArraySetAsSeries(_k, false);

   SetIndexBuffer(0,ExtMainBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtSignalBuffer,INDICATOR_DATA);
   SetIndexBuffer(2,_k, INDICATOR_CALCULATIONS);
//--- set levels
   IndicatorSetInteger(INDICATOR_LEVELS,2);
   IndicatorSetDouble(INDICATOR_LEVELVALUE,0,20);
   IndicatorSetDouble(INDICATOR_LEVELVALUE,1,80);
//--- set maximum and minimum for subwindow
   IndicatorSetDouble(INDICATOR_MINIMUM,0);
   IndicatorSetDouble(INDICATOR_MAXIMUM,100);
//--- name for DataWindow and indicator subwindow label
   IndicatorSetString(INDICATOR_SHORTNAME,"Stochastic("+(string)InpKPeriod+","+(string)InpDPeriod+","+(string)InpSlowing+")");
   PlotIndexSetString(0,PLOT_LABEL,"Main");
   PlotIndexSetString(1,PLOT_LABEL,"Signal");
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,InpKPeriod+InpSlowing-2);
   PlotIndexSetInteger(1,PLOT_DRAW_BEGIN,InpKPeriod+InpDPeriod);

   switch(InpAppliedMA){
      case MODE_SMA:
      case MODE_EMA:
      case MODE_SMMA:
      case MODE_LWMA:
         AppliedMA = InpAppliedMA;
         break;
      default:
         Print("Incorrect MA method selected - '" + IntegerToString(InpAppliedMA) + "'. Using SMA...");
         AppliedMA = MODE_SMA;
         break;
   }

   if(InpAppliedPrice == STO_LOWHIGH || InpAppliedPrice == STO_CLOSECLOSE){
      AppliedPrice = InpAppliedPrice;
   }
   else {
      Print("Incorrect applied price selected - '" + IntegerToString(InpAppliedMA) + "'. Using Low/High...");
      AppliedPrice = STO_LOWHIGH;
   }

//--- initialization done
  }
//+------------------------------------------------------------------+
//| Stochastic Oscillator                                            |
//+------------------------------------------------------------------+
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

   ArraySetAsSeries(time, false);
   ArraySetAsSeries(open, false);
   ArraySetAsSeries(high, false);
   ArraySetAsSeries(low, false);
   ArraySetAsSeries(close, false);

   double nom, den;
   int limit;

   if(prev_calculated == 0 || prev_calculated < 0 || prev_calculated > rates_total){
      limit = 0;
   }
   else {
      limit = prev_calculated - 1;
   }

   for(int i=limit; i<rates_total; i++){
      if(AppliedPrice == STO_LOWHIGH){
         nom = NormalizeDouble(close[i] - lowest(low, i), 8);
         den = NormalizeDouble(highest(high, i) - lowest(low, i), 8);
      }
      else {
         nom = NormalizeDouble(close[i] - lowest(close, i), 8);
         den = NormalizeDouble(highest(close, i) - lowest(close, i), 8);
      }

      if(den < 0.00000001 && den > -0.00000001){
         _k[i] = i == 0 ? 50 : _k[i-1];
      } else {
         _k[i] = MathMin(100, MathMax(0, 100 * nom / den));
      }

      switch(AppliedMA){
         case MODE_SMA:
            ExtMainBuffer[i] = SimpleMA(i, InpSlowing, _k);
            ExtSignalBuffer[i] = SimpleMA(i, InpDPeriod, ExtMainBuffer);
            break;
         case MODE_EMA:
            ExtMainBuffer[i] = ExponentialMA(i, InpSlowing, i == 0 ? 50 : ExtMainBuffer[i-1], _k);
            ExtSignalBuffer[i] = ExponentialMA(i, InpDPeriod, i == 0 ? 50 : ExtSignalBuffer[i-1], ExtMainBuffer);
            break;
         case MODE_SMMA:
            ExtMainBuffer[i] = SmoothedMA(i, InpSlowing, i == 0 ? 50 : ExtMainBuffer[i-1], _k);
            ExtSignalBuffer[i] = SmoothedMA(i, InpDPeriod, i == 0 ? 50 : ExtSignalBuffer[i-1], ExtMainBuffer);
            break;
         case MODE_LWMA:
            ExtMainBuffer[i] = LinearWeightedMA(i, InpSlowing, _k);
            ExtSignalBuffer[i] = LinearWeightedMA(i, InpDPeriod, ExtMainBuffer);
            break;

      }
   }
//--- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+

double highest(const double &price[], int index){
   if(index < InpKPeriod + 1){
      return 50;
   }
   else {
      double highestValue = -1;

      for(int a=index-InpKPeriod+1; a<=index; a++){
         if(price[a] - highestValue > 0.00000001){
            highestValue = price[a];
         }
      }

      return highestValue;
   }
}

//+------------------------------------------------------------------+

double lowest(const double &price[], int index){
   if(index < InpKPeriod + 1){
      return 50;
   }
   else {
      double lowestValue = 100000000000000;

      for(int a=index-InpKPeriod+1; a<=index; a++){
         if(lowestValue - price[a] > 0.00000001){
            lowestValue = price[a];
         }
      }

      return lowestValue;
   }
}
"#.to_string()
}

// ── BT_CCI ──

fn gen_mql5_cci() -> String {
r#"//+------------------------------------------------------------------+
//|                                                        BT_CCI.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Commodity Channel Index"
#include <MovingAverages.mqh>
//---
#property indicator_separate_window
#property indicator_buffers       4
#property indicator_plots         1
#property indicator_type1         DRAW_LINE
#property indicator_color1        LightSeaGreen
#property indicator_level1       -100.0
#property indicator_level2        100.0
//--- input parametrs
input int  InpCCIPeriod=14; // Period
input int  InpPrice=PRICE_TYPICAL; // Applied price
//--- global variable
int        ExtCCIPeriod, ExtCCIPrice;
//---- indicator buffer
double     ExtSPBuffer[];
double     ExtDBuffer[];
double     ExtMBuffer[];
double     ExtCCIBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- check for input value of period
   if(InpCCIPeriod<=0)
     {
      ExtCCIPeriod=14;
      printf("Incorrect value for input variable InpCCIPeriod=%d. Indicator will use value=%d for calculations.",InpCCIPeriod,ExtCCIPeriod);
     }
   else ExtCCIPeriod=InpCCIPeriod;

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         ExtCCIPrice = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_HIGH for calculations.",InpPrice);
         ExtCCIPrice = PRICE_TYPICAL;
   }

//--- define buffers
   SetIndexBuffer(0,ExtCCIBuffer);
   SetIndexBuffer(1,ExtDBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(2,ExtMBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(3,ExtSPBuffer,INDICATOR_CALCULATIONS);
//--- indicator name
   IndicatorSetString(INDICATOR_SHORTNAME,"CCI("+string(ExtCCIPeriod)+")");
//--- indexes draw begin settings
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ExtCCIPeriod-1);
//--- number of digits of indicator value
   IndicatorSetInteger(INDICATOR_DIGITS,2);
//---- OnInit done
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- variables
   int    i,j;
   double dTmp,dMul=0.015/ExtCCIPeriod;
//--- start calculation
   int StartCalcPosition=ExtCCIPeriod-1;
//--- check for bars count
   if(rates_total<StartCalcPosition)
      return(0);
//--- correct draw begin
   if(prev_calculated>0) PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,StartCalcPosition+(ExtCCIPeriod-1));
//--- calculate position
   int pos=prev_calculated-1;
   if(pos<StartCalcPosition)
      pos=StartCalcPosition;
//--- main cycle
   for(i=pos;i<rates_total && !IsStopped();i++)
     {

      //--- SMA on price buffer
      double sma=0.0;
      //--- check position
      if(i>=ExtCCIPeriod-1 && ExtCCIPeriod>0){
         //--- calculate value
         for(int a=0;a<ExtCCIPeriod;a++) sma+=getValue(open,high,low,close,ExtCCIPrice,i-a);
         sma/=ExtCCIPeriod;
      }

      ExtSPBuffer[i]=sma;
      //--- calculate D
      dTmp=0.0;
      for(j=0;j<ExtCCIPeriod;j++) dTmp+=MathAbs(getValue(open,high,low,close,ExtCCIPrice,i-j)-ExtSPBuffer[i]);
      ExtDBuffer[i]=dTmp*dMul;
      //--- calculate M
      ExtMBuffer[i]=getValue(open,high,low,close,ExtCCIPrice,i)-ExtSPBuffer[i];
      //--- calculate CCI
      if(ExtDBuffer[i] < 0.0000000001) ExtCCIBuffer[i]=0.0;
      else                             ExtCCIBuffer[i]=ExtMBuffer[i]/ExtDBuffer[i];
      //---
     }
//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+

double getValue(const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                int priceMode,
                int index)
{
   switch(priceMode){
      case PRICE_OPEN: return open[index];
      case PRICE_HIGH: return high[index];
      case PRICE_LOW: return low[index];
      case PRICE_CLOSE: return close[index];
      case PRICE_MEDIAN: return (high[index] + low[index]) / 2;
      case PRICE_TYPICAL: return (high[index] + low[index] + close[index]) / 3;
      case PRICE_WEIGHTED: return (high[index] + low[index] + close[index] + close[index]) / 4;
      default: return 0;
   }
}
"#.to_string()
}

// ── BT_WilliamsR ──

fn gen_mql5_wpr() -> String {
r#"//+------------------------------------------------------------------+
//|                                                  BT_WilliamsR.mq5 |
//|                   Copyright 2009-2020, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright   "2009-2020, MetaQuotes Software Corp."
#property link        "http://www.mql5.com"
#property description "Larry Williams' Percent Range"
//--- indicator settings
#property indicator_separate_window
#property indicator_level1     -20.0
#property indicator_level2     -80.0
#property indicator_levelstyle STYLE_DOT
#property indicator_levelcolor Silver
#property indicator_levelwidth 1
#property indicator_maximum    0.0
#property indicator_minimum    -100.0
#property indicator_buffers    1
#property indicator_plots      1
#property indicator_type1      DRAW_LINE
#property indicator_color1     DodgerBlue
//--- input parameters
input int InpWPRPeriod=14; // Period
//--- indicator buffers
double    ExtWPRBuffer[];
//--- global variables
int       ExtPeriodWPR;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- check for input value
   if(InpWPRPeriod<3)
     {
      ExtPeriodWPR=14;
      Print("Incorrect InpWPRPeriod value. Indicator will use value=",ExtPeriodWPR);
     }
   else
      ExtPeriodWPR=InpWPRPeriod;
//--- indicator's buffer
   SetIndexBuffer(0,ExtWPRBuffer);
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ExtPeriodWPR-1);
//--- name for DataWindow and indicator subwindow label
   IndicatorSetString(INDICATOR_SHORTNAME,"%R"+"("+string(ExtPeriodWPR)+")");
//--- digits
   IndicatorSetInteger(INDICATOR_DIGITS,2);
  }
//+------------------------------------------------------------------+
//| Williams' Percent Range                                          |
//+------------------------------------------------------------------+
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
   if(rates_total<ExtPeriodWPR)
      return(0);
//--- start working
   int i,pos=prev_calculated-1;
   if(pos<ExtPeriodWPR-1)
     {
      pos=ExtPeriodWPR-1;
      for(i=0; i<pos; i++)
         ExtWPRBuffer[i]=0.0;
     }
//---  main cycle
   for(i=pos; i<rates_total && !IsStopped(); i++)
     {
      double max_high=Highest(high,ExtPeriodWPR,i);
      double min_low =Lowest(low,ExtPeriodWPR,i);
      //--- calculate WPR
      if(max_high!=min_low)
         ExtWPRBuffer[i]=-(max_high-close[i])*100/(max_high-min_low);
      else
         ExtWPRBuffer[i]=ExtWPRBuffer[i-1];
     }
//--- return new prev_calculated value
   return(rates_total);
  }
//+------------------------------------------------------------------+
//| Maximum High                                                     |
//+------------------------------------------------------------------+
double Highest(const double &array[],int period,int cur_position)
  {
   double res=array[cur_position];
   for(int i=cur_position-1; i>cur_position-period && i>=0; i--)
      if(res<array[i])
         res=array[i];
   return(res);
  }
//+------------------------------------------------------------------+
//| Minimum Low                                                      |
//+------------------------------------------------------------------+
double Lowest(const double &array[],int period,int cur_position)
  {
   double res=array[cur_position];
   for(int i=cur_position-1; i>cur_position-period && i>=0; i--)
      if(res>array[i])
         res=array[i];
   return(res);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_ParabolicSAR ──

fn gen_mql5_parabolic_sar() -> String {
r#"//+------------------------------------------------------------------+
//|                                               BT_ParabolicSAR.mq5 |
//|                   Copyright 2009-2017, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "2009-2017, MetaQuotes Software Corp."
#property link      "http://www.mql5.com"
//--- indicator settings
#property indicator_chart_window
#property indicator_buffers 3
#property indicator_plots   1
#property indicator_type1   DRAW_ARROW
#property indicator_color1  DodgerBlue
//--- External parametrs
input double         InpSARStep=0.02;    // Step
input double         InpSARMaximum=0.2;  // Maximum
//---- buffers
double               ExtSARBuffer[];
double               ExtEPBuffer[];
double               ExtAFBuffer[];
//--- global variables
int                  ExtLastRevPos;
bool                 ExtDirectionLong;
double               ExtSarStep;
double               ExtSarMaximum;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- checking input data
   if(InpSARStep<0.0)
     {
      ExtSarStep=0.02;
      Print("Input parametr InpSARStep has incorrect value. Indicator will use value",
            ExtSarStep,"for calculations.");
     }
   else ExtSarStep=InpSARStep;
   if(InpSARMaximum<0.0)
     {
      ExtSarMaximum=0.2;
      Print("Input parametr InpSARMaximum has incorrect value. Indicator will use value",
            ExtSarMaximum,"for calculations.");
     }
   else ExtSarMaximum=InpSARMaximum;
//---- indicator buffers
   SetIndexBuffer(0,ExtSARBuffer);
   SetIndexBuffer(1,ExtEPBuffer, INDICATOR_CALCULATIONS);
   SetIndexBuffer(2,ExtAFBuffer, INDICATOR_CALCULATIONS);
//--- set arrow symbol
   PlotIndexSetInteger(0,PLOT_ARROW,159);
//--- set indicator digits
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- set label name
   PlotIndexSetString(0,PLOT_LABEL,"SAR("+
                      DoubleToString(ExtSarStep,2)+","+
                      DoubleToString(ExtSarMaximum,2)+")");
//--- set global variables
   ExtLastRevPos=0;
   ExtDirectionLong=false;
//----
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- check for minimum rates count
   if(rates_total<3)
      return(0);
//--- detect current position
   int pos=prev_calculated-1;
//--- correct position
   if(pos<1)
     {
      //--- first pass, set as SHORT
      pos=1;
      ExtAFBuffer[0]=ExtSarStep;
      ExtAFBuffer[1]=ExtSarStep;
      ExtSARBuffer[0]=high[0];
      ExtLastRevPos=0;
      ExtDirectionLong=false;
      ExtSARBuffer[1]=GetHigh(pos,ExtLastRevPos,high);
      ExtEPBuffer[0]=low[pos];
      ExtEPBuffer[1]=low[pos];
     }
//---main cycle
   for(int i=pos;i<rates_total-1 && !IsStopped();i++)
     {
      //--- check for reverse
      if(ExtDirectionLong)
        {
         if(ExtSARBuffer[i]>low[i])
           {
            //--- switch to SHORT
            ExtDirectionLong=false;
            ExtSARBuffer[i]=GetHigh(i,ExtLastRevPos,high);
            ExtEPBuffer[i]=low[i];
            ExtLastRevPos=i;
            ExtAFBuffer[i]=ExtSarStep;
           }
        }
      else
        {
         if(ExtSARBuffer[i]<high[i])
           {
            //--- switch to LONG
            ExtDirectionLong=true;
            ExtSARBuffer[i]=GetLow(i,ExtLastRevPos,low);
            ExtEPBuffer[i]=high[i];
            ExtLastRevPos=i;
            ExtAFBuffer[i]=ExtSarStep;
           }
        }
      //--- continue calculations
      if(ExtDirectionLong)
        {
         //--- check for new High
         if(high[i]>ExtEPBuffer[i-1] && i!=ExtLastRevPos)
           {
            ExtEPBuffer[i]=high[i];
            ExtAFBuffer[i]=ExtAFBuffer[i-1]+ExtSarStep;
            if(ExtAFBuffer[i]>ExtSarMaximum)
               ExtAFBuffer[i]=ExtSarMaximum;
           }
         else
           {
            //--- when we haven't reversed
            if(i!=ExtLastRevPos)
              {
               ExtAFBuffer[i]=ExtAFBuffer[i-1];
               ExtEPBuffer[i]=ExtEPBuffer[i-1];
              }
           }
         //--- calculate SAR for tomorrow
         ExtSARBuffer[i+1]=ExtSARBuffer[i]+ExtAFBuffer[i]*(ExtEPBuffer[i]-ExtSARBuffer[i]);
         //--- check for SAR
         if(ExtSARBuffer[i+1]>low[i] || ExtSARBuffer[i+1]>low[i-1])
            ExtSARBuffer[i+1]=MathMin(low[i],low[i-1]);
        }
      else
        {
         //--- check for new Low
         if(low[i]<ExtEPBuffer[i-1] && i!=ExtLastRevPos)
           {
            ExtEPBuffer[i]=low[i];
            ExtAFBuffer[i]=ExtAFBuffer[i-1]+ExtSarStep;
            if(ExtAFBuffer[i]>ExtSarMaximum)
               ExtAFBuffer[i]=ExtSarMaximum;
           }
         else
           {
            //--- when we haven't reversed
            if(i!=ExtLastRevPos)
              {
               ExtAFBuffer[i]=ExtAFBuffer[i-1];
               ExtEPBuffer[i]=ExtEPBuffer[i-1];
              }
           }
         //--- calculate SAR for tomorrow
         ExtSARBuffer[i+1]=ExtSARBuffer[i]+ExtAFBuffer[i]*(ExtEPBuffer[i]-ExtSARBuffer[i]);
         //--- check for SAR
         if(ExtSARBuffer[i+1]<high[i] || ExtSARBuffer[i+1]<high[i-1])
            ExtSARBuffer[i+1]=MathMax(high[i],high[i-1]);
        }
     }
//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+
//| Find highest price from start to current position                |
//+------------------------------------------------------------------+
double GetHigh(int nPosition,int nStartPeriod,const double &HiData[])
  {
//--- calculate
   double result=HiData[nStartPeriod];
   for(int i=nStartPeriod;i<=nPosition;i++) if(result<HiData[i]) result=HiData[i];
   return(result);
  }
//+------------------------------------------------------------------+
//| Find lowest price from start to current position                 |
//+------------------------------------------------------------------+
double GetLow(int nPosition,int nStartPeriod,const double &LoData[])
  {
//--- calculate
   double result=LoData[nStartPeriod];
   for(int i=nStartPeriod;i<=nPosition;i++) if(result>LoData[i]) result=LoData[i];
   return(result);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_ROC ──

fn gen_mql5_roc() -> String {
r#"//+------------------------------------------------------------------+
//|                                                        BT_ROC.mq5 |
//|                           Copyright © 2022, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2022, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Price Rate Of Change"
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots 1
#property indicator_type1   DRAW_LINE
#property indicator_color1  DodgerBlue
#property indicator_label1  "SqROC"
//--- input parameters
input int InpRocPeriod=5;  // ROC period
//--- indicator buffers
double    ExtROCBuffer[];
//--- global variable
int       ExtPeriodROC;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- check for input value
   if(InpRocPeriod<=0)
     {
      ExtPeriodROC=5;
      printf("Incorrect input parameter InpRocPeriod = %d. Indicator will use value %d for calculations.",InpRocPeriod,ExtPeriodROC);
     }
   else ExtPeriodROC=InpRocPeriod;

//--- indicator buffers mapping

   ArraySetAsSeries(ExtROCBuffer, false);
   SetIndexBuffer(0,ExtROCBuffer,INDICATOR_DATA);

//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- name for DataWindow and indicator subwindow label
   string short_name="ROC("+string(ExtPeriodROC)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//--- initialization done
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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

   ArraySetAsSeries(time, false);
   ArraySetAsSeries(open, false);
   ArraySetAsSeries(high, false);
   ArraySetAsSeries(low, false);
   ArraySetAsSeries(close, false);

   int i,limit;
//--- check for bars count
   if(rates_total<=ExtPeriodROC)
      return(0); // not enough bars for calculation
//--- preliminary calculations
   if(prev_calculated==0){
      ExtROCBuffer[0] = 0;

      limit = 1;
   }
   else limit=prev_calculated-1;
//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++){
      double prevClose = i >= ExtPeriodROC ? close[i-ExtPeriodROC] : 0;
      if(prevClose == 0){
         ExtROCBuffer[i] = 0;
      }
      else {
         double roc = (close[i] - prevClose) / prevClose * 100;
         ExtROCBuffer[i] = roc;
      }
   }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_Ichimoku ──

fn gen_mql5_ichimoku() -> String {
r#"//+------------------------------------------------------------------+
//|                                                   BT_Ichimoku.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Ichimoku Kinko Hyo"
//--- indicator settings
#property indicator_chart_window
#property indicator_buffers 5
#property indicator_plots   4
#property indicator_type1   DRAW_LINE
#property indicator_type2   DRAW_LINE
#property indicator_type3   DRAW_FILLING
#property indicator_type4   DRAW_LINE
#property indicator_color1  Red
#property indicator_color2  Blue
#property indicator_color3  SandyBrown,Thistle
#property indicator_color4  Lime
#property indicator_label1  "Tenkan-sen"
#property indicator_label2  "Kijun-sen"
#property indicator_label3  "Senkou Span A;Senkou Span B"
#property indicator_label4  "Chikou Span"
//--- input parameters
input int InpTenkan=9;     // Tenkan-sen
input int InpKijun=26;     // Kijun-sen
input int InpSenkou=52;    // Senkou Span B
//--- indicator buffers
double    ExtTenkanBuffer[];
double    ExtKijunBuffer[];
double    ExtSpanABuffer[];
double    ExtSpanBBuffer[];
double    ExtChikouBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtTenkanBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtKijunBuffer,INDICATOR_DATA);
   SetIndexBuffer(2,ExtSpanABuffer,INDICATOR_DATA);
   SetIndexBuffer(3,ExtSpanBBuffer,INDICATOR_DATA);
   SetIndexBuffer(4,ExtChikouBuffer,INDICATOR_DATA);
//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits+1);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,InpTenkan);
   PlotIndexSetInteger(1,PLOT_DRAW_BEGIN,InpKijun);
   PlotIndexSetInteger(2,PLOT_DRAW_BEGIN,InpSenkou-1);
//--- lines shifts when drawing
   PlotIndexSetInteger(2,PLOT_SHIFT,InpKijun);
//--- change labels for DataWindow
   PlotIndexSetString(0,PLOT_LABEL,"Tenkan-sen("+string(InpTenkan)+")");
   PlotIndexSetString(1,PLOT_LABEL,"Kijun-sen("+string(InpKijun)+")");
   PlotIndexSetString(2,PLOT_LABEL,"Senkou Span A;Senkou Span B("+string(InpSenkou)+")");
//--- initialization done
  }
//+------------------------------------------------------------------+
//| get highest value for range                                      |
//+------------------------------------------------------------------+
double Highest(const double&array[],int range,int fromIndex)
  {
   double res=0;
//---
   res=array[fromIndex];
   for(int i=fromIndex;i>fromIndex-range && i>=0;i--)
     {
      if(res<array[i]) res=array[i];
     }
//---
   return(res);
  }
//+------------------------------------------------------------------+
//| get lowest value for range                                       |
//+------------------------------------------------------------------+
double Lowest(const double&array[],int range,int fromIndex)
  {
   double res=0;
//---
   res=array[fromIndex];
   for(int i=fromIndex;i>fromIndex-range && i>=0;i--)
     {
      if(res>array[i]) res=array[i];
     }
//---
   return(res);
  }
//+------------------------------------------------------------------+
//| Ichimoku Kinko Hyo                                               |
//+------------------------------------------------------------------+
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
   int limit;
//---
   if(prev_calculated==0) limit=0;
   else                   limit=prev_calculated-1;
//---
   int chikouStart = rates_total > InpKijun ? rates_total - InpKijun : 0;

   for(int a=chikouStart; a<rates_total; a++){
      ExtChikouBuffer[a]=close[rates_total-1];
   }

   for(int i=limit;i<rates_total && !IsStopped();i++)
     {

      ExtChikouBuffer[i]=close[rates_total-1];
      if(i >= InpKijun){
         ExtChikouBuffer[i-InpKijun]=close[i];
      }

      //--- tenkan sen
      double _high=Highest(high,InpTenkan,i);
      double _low=Lowest(low,InpTenkan,i);
      ExtTenkanBuffer[i]=(_high+_low)/2.0;
      //--- kijun sen
      _high=Highest(high,InpKijun,i);
      _low=Lowest(low,InpKijun,i);
      ExtKijunBuffer[i]=(_high+_low)/2.0;
      //--- senkou span a
      ExtSpanABuffer[i]=(ExtTenkanBuffer[i]+ExtKijunBuffer[i])/2.0;
      //--- senkou span b
      _high=Highest(high,InpSenkou,i);
      _low=Lowest(low,InpSenkou,i);
      ExtSpanBBuffer[i]=(_high+_low)/2.0;
     }
//--- done
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_KeltnerChannel ──

fn gen_mql5_keltner_channel() -> String {
r#"//+------------------------------------------------------------------+
//|                                            BT_KeltnerChannel.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright "Copyright © 2017, StrategyQuant s.r.o."
#property link      "http://www.strategyquant.com"
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
#property indicator_chart_window
#property indicator_buffers 3
#property indicator_plots 3

#property indicator_label1  "Upper"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Blue

#property indicator_label2  "Lower"
#property indicator_type2  DRAW_LINE
#property indicator_color2 Red

#property indicator_label3  "Middle"
#property indicator_type3  DRAW_LINE
#property indicator_color3 White


double upper[], middle[], lower[];
input int     MAPeriod = 20;
input double  Const = 1.5;

int period;

void OnInit()
  {
//--- check for input parameters
   if(MAPeriod <= 0){
      printf("Incorrect value for input variable MAPeriod=%d. Indicator will use value=%d for calculations.", MAPeriod, 14);
      period = 14;
   }
   else period = MAPeriod;

   ArraySetAsSeries(upper, true);
   ArraySetAsSeries(lower, true);
   ArraySetAsSeries(middle, true);

   SetIndexBuffer(0, upper);
   SetIndexBuffer(1, lower);
   SetIndexBuffer(2, middle);

   PlotIndexSetInteger(1, PLOT_LINE_STYLE, STYLE_DOT);

//--- indicator short name
   string short_name="SqKeltnerChannel("+string(period)+","+string(Const)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
}

//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- checking for bars count
   if(rates_total < period) return(0);

   ArraySetAsSeries(time, true);
   ArraySetAsSeries(open, true);
   ArraySetAsSeries(high, true);
   ArraySetAsSeries(low, true);
   ArraySetAsSeries(close, true);

   int limit;
   double offset;

   if(prev_calculated > 0) limit = rates_total - prev_calculated + 1;
   else {
      for(int a=0; a<rates_total; a++){
         upper[a] = 0.0;
         middle[a] = 0.0;
         lower[a] = 0.0;
      }

      limit = rates_total - period;
   }

   for(int x=0; x<limit && !IsStopped(); x++) {
      offset = avgDiff(high, low, period, x) * Const;

      middle[x] = avgTrueRange(high, low, close, period, x);
      upper[x] = middle[x] + offset;
      lower[x] = middle[x] - offset;
   }

   return(rates_total);
}

//+------------------------------------------------------------------+

double avgTrueRange(const double &high[], const double &low[], const double &close[], int atrPeriod, int shift) {
  double sum=0;
  for (int x=shift;x<(shift+atrPeriod);x++) {
     sum += (high[x] + low[x] + close[x]) / 3;
  }

  sum = sum / atrPeriod;
  return (sum);
}

double avgDiff(const double &high[], const double &low[], int atrPeriod, int shift) {
  double sum=0;
  for (int x=shift;x<(shift+atrPeriod);x++) {
     sum += high[x] - low[x];
  }

  sum = sum / atrPeriod;
  return (sum);
}


double getIndicatorValue(int indyHandle, int bufferIndex, int shift){
   double buffer[];

   if(CopyBuffer(indyHandle, bufferIndex, shift, 1, buffer) < 0) {
      PrintFormat("Failed to copy data from the indicator, error code %d", GetLastError());
      return(0);
   }

   return buffer[0];
}
"#.to_string()
}

// ── BT_SuperTrend ──

fn gen_mql5_supertrend() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_SuperTrend.mq5 |
//|                            Copyright c @2021 StrategyQuant s.r.o.|
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property  copyright "Copyright c @2021 StrategyQuant s.r.o."
#property  link      "http://www.strategyquant.com"

#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "SqSuperTrend"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Red

//---- indicator parameters
input int    STMode=1;
input int    ATRPeriod=24;
input double ATRMultiplication=3;

//---- internal periods
int inSTMode;
double inATRMultiplication;
int inATRPeriod;
//---- buffers
double ind_buffer[];
//---- handle
int atrHandle;

void OnInit()
  {
   // Refer to SQX Supertrend.java, Mode wasn't used
   inSTMode = 1;
   if(ATRPeriod <= 1 ){
      printf("Incorrect value for input variable ATRPeriod=%d. Indicator will use value=%d for calculations.", ATRPeriod, 24);
      inATRPeriod = 24;
   }
   else inATRPeriod = ATRPeriod;

   if(ATRMultiplication <= 0 ){
      printf("Incorrect value for input variable ATRMultiplication=%d. Indicator will use value=%d for calculations.", ATRMultiplication, 3);
      inATRMultiplication = 3;
   }
   else inATRMultiplication = (double)ATRMultiplication;



   ArraySetAsSeries(ind_buffer, true);
   SetIndexBuffer(0, ind_buffer,INDICATOR_DATA);
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,inATRPeriod);

   atrHandle = iATR(NULL,0,inATRPeriod);

//--- indicator short name
   string short_name="SqSRPercRank("+string(inATRPeriod)+","+string(inATRMultiplication)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
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
   ArraySetAsSeries(open, true);
   ArraySetAsSeries(high, true);
   ArraySetAsSeries(low, true);
   ArraySetAsSeries(close, true);

   if(rates_total < ATRPeriod) return(0);

   int limit;

   if(prev_calculated > 0) limit = rates_total - prev_calculated + 1;
   else {
      for(int a=0; a<rates_total; a++){
         ind_buffer[a] = 0.0;
      }

      limit = rates_total - inATRPeriod;
   }
 //--- main indicator loop

   for(int i=limit-1; i>=0; i--) {

      if(inSTMode == 1){


          double dAtr = getIndicatorValue(atrHandle, 0, i);
          double dUpperLevel=(high[i]+low[i])/2+inATRMultiplication*dAtr;
          double dLowerLevel=(high[i]+low[i])/2-inATRMultiplication*dAtr;

          if(close[i]>ind_buffer[i+1] && close[i+1]<=ind_buffer[i+1]){
          ind_buffer[i] = dLowerLevel;
          }
          else if(close[i]<ind_buffer[i+1] && close[i+1]>=ind_buffer[i+1]){
            ind_buffer[i] = dUpperLevel;
          }
          else if(ind_buffer[i+1]<dLowerLevel){
            ind_buffer[i] = dLowerLevel;
          }
          else if(ind_buffer[i+1]>dUpperLevel){
            ind_buffer[i] = dUpperLevel;
          }
          else ind_buffer[i] = ind_buffer[i+1];

          }
   }

   return(rates_total);

  }
//+------------------------------------------------------------------+



double getIndicatorValue(int indyHandle, int bufferIndex, int shift){
   double buffer[];

   if(CopyBuffer(indyHandle, bufferIndex, shift, 1, buffer) < 0) {
      PrintFormat("Failed to copy data from the indicator, error code %d", GetLastError());
      return(0);
   }

   double val = buffer[0];
   return val;
}
"#.to_string()
}

// ── BT_LaguerreRSI ──

fn gen_mql5_laguerre_rsi() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_LaguerreRSI.mq5 |
//|                             Copyright c 2010,   Nikolay Kositsin |
//|                              Khabarovsk,   farria@mail.redcom.ru |
//+------------------------------------------------------------------+
#property copyright "Copyright c 2010, Nikolay Kositsin"
#property link "farria@mail.redcom.ru"
//--- indicator version
#property version   "1.00"
//--- drawing the indicator in a separate window
#property indicator_separate_window
//--- one buffer is used for calculation and drawing of the indicator
#property indicator_buffers 1
//--- only one plot is used
#property indicator_plots   1
//--- drawing of the indicator as a line
#property indicator_type1   DRAW_LINE
//--- Magenta color is used for the indicator line
#property indicator_color1  Magenta
//--- values of indicator's horizontal levels
#property indicator_level2 0.75
#property indicator_level3 0.45
#property indicator_level4 0.15
//--- blue color is used as the color of the horizontal level
#property indicator_levelcolor Blue
//--- line style
#property indicator_levelstyle STYLE_DASHDOTDOT
//--- indicator input parameters
input double gamma=0.7;
//--- declaration of dynamic array that further
//--- will be used as indicator buffers
double ExtLineBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- set ExtLineBuffer[] dynamic array as indicator buffer
   SetIndexBuffer(0,ExtLineBuffer,INDICATOR_DATA);
//--- prepare a variable for indicator short name
   string shortname;
   StringConcatenate(shortname,"Laguerre(",gamma,")");
//--- create label to display in Data Window
   PlotIndexSetString(0,PLOT_LABEL,shortname);
//--- creating name for displaying in a separate sub-window and in a tooltip
   IndicatorSetString(INDICATOR_SHORTNAME,shortname);
//--- set accuracy of displaying of the indicator values
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits+1);
//--- set empty values for the indicator
   PlotIndexSetDouble(0,PLOT_EMPTY_VALUE,EMPTY_VALUE);
//---
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
int OnCalculate(const int rates_total,    // number of bars in history at the current tick
                const int prev_calculated,// number of bars, calculated at previous call
                const int begin,          // number of beginning of reliable counting of bars
                const double &price[])    // price array for calculation of the indicator
  {
//--- checking the number of bars to be enough for the calculation
   if(rates_total<begin) return(0);
//--- declarations of local variables
   int first,bar;
   double L0,L1,L2,L3,L0A,L1A,L2A,L3A,LRSI=0,CU,CD;
//--- declaration of static variables for storing real values of coefficients
   static double L0_,L1_,L2_,L3_,L0A_,L1A_,L2A_,L3A_;
//--- calculation of the starting number 'first' for the cycle of recalculation of bars
   if(prev_calculated>rates_total || prev_calculated<=0) // checking for the first start of calculation of an indicator
     {
      first=begin; // starting number for calculation of all bars
      //--- the starting initialization of calculated coefficients
      L0_ = price[first];
      L1_ = price[first];
      L2_ = price[first];
      L3_ = price[first];
      L0A_ = price[first];
      L1A_ = price[first];
      L2A_ = price[first];
      L3A_ = price[first];
     }
   else first=prev_calculated-1; // starting number for calculation of new bars
//--- restore values of the variables
   L0 = L0_;
   L1 = L1_;
   L2 = L2_;
   L3 = L3_;
   L0A = L0A_;
   L1A = L1A_;
   L2A = L2A_;
   L3A = L3A_;

//--- main cycle of calculation of the indicator
   for(bar=first; bar<rates_total; bar++)
     {
      //--- memorize values of the variables before running at the current bar
      if(rates_total!=prev_calculated && bar==rates_total-1)
        {
         L0_ = L0;
         L1_ = L1;
         L2_ = L2;
         L3_ = L3;
         L0A_ = L0A;
         L1A_ = L1A;
         L2A_ = L2A;
         L3A_ = L3A;
        }

      L0A = L0;
      L1A = L1;
      L2A = L2;
      L3A = L3;
      //---
      L0 = (1 - gamma) * price[bar] + gamma * L0A;
      L1 = - gamma * L0 + L0A + gamma * L1A;
      L2 = - gamma * L1 + L1A + gamma * L2A;
      L3 = - gamma * L2 + L2A + gamma * L3A;
      //---
      CU = 0;
      CD = 0;
      //---
      if(L0 >= L1) CU  = L0 - L1; else CD  = L1 - L0;
      if(L1 >= L2) CU += L1 - L2; else CD += L2 - L1;
      if(L2 >= L3) CU += L2 - L3; else CD += L3 - L2;
      //---
      if(CU+CD!=0) LRSI=CU/(CU+CD);

      //--- set value to ExtLineBuffer[]
      ExtLineBuffer[bar]=LRSI;
     }
//---
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_BearsPower ──

fn gen_mql5_bears_power() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_BearsPower.mq5 |
//|                   Copyright 2009-2017, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright   "2009-2017, MetaQuotes Software Corp."
#property link        "http://www.mql5.com"
#property description "Bears Power"
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   1
#property indicator_type1   DRAW_HISTOGRAM
#property indicator_color1  Silver
#property indicator_width1  2
//--- input parameters
input int InpBearsPeriod=13; // Period
input int InpPrice=13; // Input Price
//--- indicator buffers
double    ExtBearsBuffer[];
double    ExtTempBuffer[];
//--- handle of EMA
int       ExtEmaHandle;
int mode;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtBearsBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtTempBuffer,INDICATOR_CALCULATIONS);

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_CLOSE for calculations.",InpPrice);
         mode=PRICE_CLOSE;
   }

//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits+1);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,InpBearsPeriod-1);
//--- name for DataWindow and indicator subwindow label
   IndicatorSetString(INDICATOR_SHORTNAME,"Bears("+(string)InpBearsPeriod+")");
//--- get MA handle
   ExtEmaHandle=iMA(NULL,0,InpBearsPeriod,0,MODE_EMA,mode);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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
   int i,limit;
//--- check for bars count
   if(rates_total<InpBearsPeriod)
      return(0);// not enough bars for calculation
//--- not all data may be calculated
   int calculated=BarsCalculated(ExtEmaHandle);
   if(calculated<rates_total)
     {
      Print("Not all data of ExtEmaHandle is calculated (",calculated,"bars ). Error",GetLastError());
      return(0);
     }
//--- we can copy not all data
   int to_copy;
   if(prev_calculated>rates_total || prev_calculated<0) to_copy=rates_total;
   else
     {
      to_copy=rates_total-prev_calculated;
      if(prev_calculated>0) to_copy++;
     }
//---- get ma buffers
   if(IsStopped()) return(0); //Checking for stop flag
   if(CopyBuffer(ExtEmaHandle,0,0,to_copy,ExtTempBuffer)<=0)
     {
      Print("getting ExtEmaHandle is failed! Error",GetLastError());
      return(0);
     }
//--- first calculation or number of bars was changed
   if(prev_calculated<InpBearsPeriod)
      limit=InpBearsPeriod;
   else limit=prev_calculated-1;
//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++)
     {
      ExtBearsBuffer[i]=low[i]-ExtTempBuffer[i];
     }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_BullsPower ──

fn gen_mql5_bulls_power() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_BullsPower.mq5 |
//|                   Copyright 2009-2017, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright   "2009-2017, MetaQuotes Software Corp."
#property link        "http://www.mql5.com"
#property description "Bulls Power"
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   1
#property indicator_type1   DRAW_HISTOGRAM
#property indicator_color1  Silver
#property indicator_width1  2
//--- input parameters
input int InpBullsPeriod=13; // Period
input int InpPrice=13; // Input Price
//--- indicator buffers
double    ExtBullsBuffer[];
double    ExtTempBuffer[];
//--- MA handle
int       ExtEmaHandle;
int mode;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtBullsBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtTempBuffer,INDICATOR_CALCULATIONS);

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_CLOSE for calculations.",InpPrice);
         mode=PRICE_CLOSE;
   }

//--- set accuracy
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits+1);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,InpBullsPeriod-1);
//--- name for DataWindow and indicator subwindow label
   IndicatorSetString(INDICATOR_SHORTNAME,"Bulls("+(string)InpBullsPeriod+")");
//--- get handle for MA
   ExtEmaHandle=iMA(NULL,0,InpBullsPeriod,0,MODE_EMA,mode);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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
   int i,limit;
//--- check for bars count
   if(rates_total<InpBullsPeriod)
      return(0);// not enough bars for calculation
//--- not all data may be calculated
   int calculated=BarsCalculated(ExtEmaHandle);
   if(calculated<rates_total)
     {
      Print("Not all data of ExtEmaHandle is calculated (",calculated,"bars ). Error",GetLastError());
      return(0);
     }
//--- we can copy not all data
   int to_copy;
   if(prev_calculated>rates_total || prev_calculated<0) to_copy=rates_total;
   else
     {
      to_copy=rates_total-prev_calculated;
      if(prev_calculated>0) to_copy++;
     }
//---- get ma buffers
   if(IsStopped()) return(0); //Checking for stop flag
   if(CopyBuffer(ExtEmaHandle,0,0,to_copy,ExtTempBuffer)<=0)
     {
      Print("getting ExtEmaHandle is failed! Error",GetLastError());
      return(0);
     }
//--- first calculation or number of bars was changed
   if(prev_calculated<InpBullsPeriod)
      limit=InpBullsPeriod;
   else limit=prev_calculated-1;
//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++)
     {
      ExtBullsBuffer[i]=high[i]-ExtTempBuffer[i];
     }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_TrueRange ──

fn gen_mql5_true_range() -> String {
r#"//+------------------------------------------------------------------+
//|                                                  BT_TrueRange.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "True Range"
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_type1   DRAW_LINE
#property indicator_color1  DodgerBlue
#property indicator_label1  "TrueRange"
//--- indicator buffers
double    ExtTRBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtTRBuffer,INDICATOR_DATA);
//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- name for DataWindow and indicator subwindow label
   string short_name="TrueRange";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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
   int i,limit;
//--- preliminary calculations
   if(prev_calculated==0){
      ExtTRBuffer[0] = high[0] - low[0];
      limit=1;
   }
   else limit=prev_calculated-1;
//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++){
      double close1 = close[i-1];
      double curHigh = high[i];
      double curLow = low[i];
      double TrueHigh, TrueLow;

      if(close1 > curHigh) {
         TrueHigh = close1;
      }
      else {
         TrueHigh = curHigh;
      }

      if(close1 < curLow) {
         TrueLow = close1;
      } else {
         TrueLow = curLow;
      }

      ExtTRBuffer[i] = TrueHigh - TrueLow;
   }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_LinearRegression ──

fn gen_mql5_linreg() -> String {
r#"//+------------------------------------------------------------------+
//|                                          BT_LinearRegression.mq5 |
//|                           Copyright © @2017 StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property  copyright "Copyright © @2017 StrategyQuant s.r.o."
#property  link      "http://www.strategyquant.com"

#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "SqLinReg"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Blue

input int LRPeriod=14;
input int InpPrice=2;

int period, mode;

double ind_buffer[];

void OnInit()
  {
   if(LRPeriod <= 0){
      printf("Incorrect value for input variable LRPeriod=%d. Indicator will use value=%d for calculations.", LRPeriod, 14);
      period = 14;
   }
   else period = LRPeriod;

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_HIGH for calculations.",InpPrice);
         mode=14;
   }

   ArraySetAsSeries(ind_buffer, true);

   SetIndexBuffer(0, ind_buffer);

//--- indicator short name
   string short_name="SqLinReg("+string(period)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
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

   if(rates_total < period) return(0);

   int limit;

   ArraySetAsSeries(time, true);
   ArraySetAsSeries(open, true);
   ArraySetAsSeries(high, true);
   ArraySetAsSeries(low, true);
   ArraySetAsSeries(close, true);

   if(prev_calculated > 0) limit = rates_total - prev_calculated + 1;
   else {
      for(int a=0; a<rates_total; a++){
         ind_buffer[a] = 0.0;
      }

      limit = rates_total - period;
   }

   for(int i=limit-1; i>=0; i--) {
      ind_buffer[i] = linreg(open, high, low, close, mode, period, i);
   }
   return(rates_total);
  }
//+------------------------------------------------------------------+

double linreg(const double &open[],
              const double &high[],
              const double &low[],
              const double &close[],
              int priceMode,
              int p,
              int i){

   double SumY=0;
   double Sum1=0;
   double Slope=0;
   double c;

   for (int x=0; x<p; x++) {
      c=getValue(open, high, low, close, priceMode, x+i);
      SumY+=c;
      Sum1+=x*c;
   }

   double SumBars=p*(p-1)*0.5;
   double SumSqrBars=(p-1)*p*(2*p-1)/6;
   double Sum2=SumBars*SumY;
   double Num1=p*Sum1-Sum2;
   double Num2=SumBars*SumBars-p*SumSqrBars;

   if(Num2!=0) Slope=Num1/Num2;
   else Slope=0;

   double Intercept=(SumY-Slope*SumBars)/p;
   double linregval=Intercept+Slope*(p-1);
   return(linregval);
}

double getValue(const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                int priceMode,
                int index)
{
   switch(priceMode){
      case PRICE_OPEN: return open[index];
      case PRICE_HIGH: return high[index];
      case PRICE_LOW: return low[index];
      case PRICE_CLOSE: return close[index];
      case PRICE_MEDIAN: return (high[index] + low[index]) / 2;
      case PRICE_TYPICAL: return (high[index] + low[index] + close[index]) / 3;
      case PRICE_WEIGHTED: return (high[index] + low[index] + close[index] + close[index]) / 4;
      default: return 0;
   }
}
"#.to_string()
}

// ── BT_Fractal ──

fn gen_mql5_fractal() -> String {
r#"//+------------------------------------------------------------------+
//|                                                    BT_Fractal.mq5 |
//|                           Copyright © 2020, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2020, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "Fractal"
//--- indicator settings
#property indicator_chart_window
#property indicator_buffers 2
#property indicator_plots   2
#property indicator_type1   DRAW_ARROW
#property indicator_type2   DRAW_ARROW
#property indicator_color1  Red
#property indicator_label1  "Fractal Up"
#property indicator_color2  Blue
#property indicator_label2  "Fractal Down"

//--- input parameters
input int Fractal=3;  // Fractal bars

//--- indicator buffers
double ExtUpFractalsBuffer[];
double ExtDownFractalsBuffer[];
//--- global variable
int FractalUsed = 3;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- check for input value
    if((Fractal - 1) / 2 <= 0){
      FractalUsed = 3;
      printf("Incorrect value for input variable Fractal=%d. Indicator will use value=%d for calculations.", Fractal, FractalUsed);
    }
    else {
      FractalUsed = Fractal;
    }
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtUpFractalsBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtDownFractalsBuffer,INDICATOR_DATA);
//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,FractalUsed);
//--- name for DataWindow and indicator subwindow label
   string short_name="Fractal";
   string short_name_up="Fractal Up";
   string short_name_down="Fractal Down";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   PlotIndexSetString(0,PLOT_LABEL,short_name_up);
   PlotIndexSetString(1,PLOT_LABEL,short_name_down);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Average True Range                                               |
//+------------------------------------------------------------------+
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
   int i,limit;
   bool   bFoundHigh, bFoundLow;
   double dCurrentHigh, dCurrentLow;

//--- check for bars count
   if(rates_total<=FractalUsed)
      return(0); // not enough bars for calculation
//--- preliminary calculations
   if(prev_calculated==0){
      ExtUpFractalsBuffer[0] = 0;
      ExtDownFractalsBuffer[0] = 0;
      limit=FractalUsed;
   }
   else limit=prev_calculated-1;
//--- the main loop of calculations

   int eachSideLength = (FractalUsed - 1) / 2;

   for(i=limit;i<rates_total && !IsStopped();i++){
      int middleBar = i - eachSideLength - 1;

      dCurrentHigh = high[middleBar];
      dCurrentLow = low[middleBar];
      bFoundHigh = true;
      bFoundLow = true;

      for(int a=i-FractalUsed; a<i; a++){
         if(a == middleBar) continue;

      //----Fractals up
         if(high[a] >= dCurrentHigh) bFoundHigh = false;
      //----Fractals down
         if(low[a] <= dCurrentLow) bFoundLow = false;
      }

      ExtUpFractalsBuffer[i]=bFoundHigh ? dCurrentHigh : 0;
      ExtDownFractalsBuffer[i]=bFoundLow ? dCurrentLow : 0;
   }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_HeikenAshi ──

fn gen_mql5_heiken_ashi() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_HeikenAshi.mq5 |
//|                   Copyright 2009-2017, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "2009-2017, MetaQuotes Software Corp."
#property link      "http://www.mql5.com"
//--- indicator settings
#property indicator_chart_window
#property indicator_buffers 5
#property indicator_plots   1
#property indicator_type1   DRAW_COLOR_CANDLES
#property indicator_color1  DodgerBlue, Red
#property indicator_label1  "Heiken Ashi Open;Heiken Ashi High;Heiken Ashi Low;Heiken Ashi Close"
//--- indicator buffers
double ExtOBuffer[];
double ExtHBuffer[];
double ExtLBuffer[];
double ExtCBuffer[];
double ExtColorBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtOBuffer,INDICATOR_DATA);
   SetIndexBuffer(1,ExtHBuffer,INDICATOR_DATA);
   SetIndexBuffer(2,ExtLBuffer,INDICATOR_DATA);
   SetIndexBuffer(3,ExtCBuffer,INDICATOR_DATA);
   SetIndexBuffer(4,ExtColorBuffer,INDICATOR_COLOR_INDEX);
//---
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- sets first bar from what index will be drawn
   IndicatorSetString(INDICATOR_SHORTNAME,"Heiken Ashi");
//--- sets drawing line empty value
   PlotIndexSetDouble(0,PLOT_EMPTY_VALUE,0.0);
//--- initialization done
  }
//+------------------------------------------------------------------+
//| Heiken Ashi                                                      |
//+------------------------------------------------------------------+
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
   int i,limit;
//--- preliminary calculations
   if(prev_calculated==0)
     {
      //--- set first candle
      ExtLBuffer[0]=low[0];
      ExtHBuffer[0]=high[0];
      ExtOBuffer[0]=open[0];
      ExtCBuffer[0]=close[0];
      limit=1;
     }
   else limit=prev_calculated-1;

   limit = MathMax(limit, 1);

//--- the main loop of calculations
   for(i=limit;i<rates_total && !IsStopped();i++)
     {
      double haOpen=(ExtOBuffer[i-1]+ExtCBuffer[i-1])/2;
      double haClose=(open[i]+high[i]+low[i]+close[i])/4;
      double haHigh=MathMax(high[i],MathMax(haOpen,haClose));
      double haLow=MathMin(low[i],MathMin(haOpen,haClose));

      ExtLBuffer[i]=haLow;
      ExtHBuffer[i]=haHigh;
      ExtOBuffer[i]=haOpen;
      ExtCBuffer[i]=haClose;

      //--- set candle color
      if(haOpen<haClose) ExtColorBuffer[i]=0.0; // set color DodgerBlue
      else               ExtColorBuffer[i]=1.0; // set color Red
     }
//--- done
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_GannHiLo ──

fn gen_mql5_gann_hi_lo() -> String {
r#"//+------------------------------------------------------------------+
//|                                                  BT_GannHiLo.mq5 |
//|                                                        avoitenko |
//|                        https://login.mql5.com/en/users/avoitenko |
//+------------------------------------------------------------------+
#property copyright     ""
#property link          "https://login.mql5.com/en/users/avoitenko"
#property version       "1.00"
#property description   "Author: Kalenzo"

#property indicator_chart_window
#property indicator_buffers   5
#property indicator_plots     1
//--- output line
#property indicator_type1  DRAW_COLOR_LINE
#property indicator_color1 clrDodgerBlue, clrOrangeRed
#property indicator_style1 STYLE_SOLID
#property indicator_width1 2

//--- input parameters
input int           InpPeriod=10;       // Period

//--- buffers
double GannBuffer[];
double ColorBuffer[];
double MaHighBuffer[];
double MaLowBuffer[];
double TrendBuffer[];
//--- global vars
int ma_high_handle;
int ma_low_handle;
int period;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- check period
   period=(int)fmax(InpPeriod,2);
//--- set buffers
   SetIndexBuffer(0,GannBuffer);
   SetIndexBuffer(1,ColorBuffer,INDICATOR_COLOR_INDEX);
   SetIndexBuffer(2,MaHighBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(3,MaLowBuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(4,TrendBuffer,INDICATOR_CALCULATIONS);
//--- set direction
   ArraySetAsSeries(GannBuffer,true);
   ArraySetAsSeries(ColorBuffer,true);
   ArraySetAsSeries(MaHighBuffer,true);
   ArraySetAsSeries(MaLowBuffer,true);
   ArraySetAsSeries(TrendBuffer,true);
//--- get handles
   ma_high_handle=iMA(NULL,0,period,0,MODE_SMA,PRICE_HIGH);
   ma_low_handle =iMA(NULL,0,period,0,MODE_SMA,PRICE_LOW);
   if(ma_high_handle==INVALID_HANDLE || ma_low_handle==INVALID_HANDLE)
     {
      Print("Unable to create handle for iMA");
      return(INIT_FAILED);
     }
//--- set indicator properties
   string short_name=StringFormat("Gann High-Low Activator SSL",period);
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
//--- set label
   short_name=StringFormat("GHL (%u, %s)",period);
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//--- done
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
   if(rates_total<period+1)return(0);
   ArraySetAsSeries(close,true);
//---
   int limit;
   if(rates_total<prev_calculated || prev_calculated<=0)
     {
      limit=rates_total-period-1;
      ArrayInitialize(GannBuffer,EMPTY_VALUE);
      ArrayInitialize(ColorBuffer,0);
      ArrayInitialize(MaHighBuffer,0);
      ArrayInitialize(MaLowBuffer,0);
      ArrayInitialize(TrendBuffer,0);
     }
   else
      limit=rates_total-prev_calculated;
//--- get MA
   if(CopyBuffer(ma_high_handle,0,0,limit+1,MaHighBuffer)!=limit+1)return(0);
   if(CopyBuffer(ma_low_handle,0,0,limit+1,MaLowBuffer)!=limit+1)return(0);
//--- main cycle
   for(int i=limit; i>=0 && !_StopFlag; i--)
     {
      TrendBuffer[i]=TrendBuffer[i+1];
      //---
      if(close[i]>MaHighBuffer[i+1]) TrendBuffer[i]=1;
      if(close[i]<MaLowBuffer[i+1]) TrendBuffer[i]=-1;
      //---
      if(TrendBuffer[i]<0)
        {
        GannBuffer[i]=MaHighBuffer[i];
         ColorBuffer[i]=1;
        }
      //---
      if(TrendBuffer[i]>0)
        {
         GannBuffer[i]=MaLowBuffer[i];
         ColorBuffer[i]=0;
        }
     }
//--- done
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_HullMA ──

fn gen_mql5_hull_ma() -> String {
r#"//+------------------------------------------------------------------+
//|                                                     BT_HullMA.mq5 |
//|                                    copyright "c mladen, 2019"     |
//|                                     mladenfx@gmail.com            |
//+------------------------------------------------------------------+
#property copyright "c mladen, 2019"
#property link      "mladenfx@gmail.com"
//------------------------------------------------------------------
#property indicator_chart_window
#property indicator_buffers 2
#property indicator_plots   1
#property indicator_label1  "Hull"
#property indicator_type1   DRAW_COLOR_LINE
#property indicator_color1  clrGray,clrMediumSeaGreen,clrOrangeRed
#property indicator_width1  2

//
//
//
//
//

input int                inpPeriod  = 20;          // Period
input double             inpDivisor = 2.0;         // Divisor ("speed")
input ENUM_APPLIED_PRICE inpPrice   = PRICE_CLOSE; // Price

double val[],valc[];

//------------------------------------------------------------------
//
//------------------------------------------------------------------
//
//
//

int OnInit()
{
   SetIndexBuffer(0,val,INDICATOR_DATA);
   SetIndexBuffer(1,valc,INDICATOR_COLOR_INDEX);
      iHull.init(inpPeriod,inpDivisor);
         IndicatorSetString(INDICATOR_SHORTNAME,"Hull ("+(string)inpPeriod+")");
   return (INIT_SUCCEEDED);
}
void OnDeinit(const int reason)
{
}

//------------------------------------------------------------------
//
//------------------------------------------------------------------
//
//
//
//
//

int OnCalculate(const int rates_total,const int prev_calculated,const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
{
   int i= prev_calculated-1; if (i<0) i=0; for (; i<rates_total && !_StopFlag; i++)
   {
      val[i]  = iHull.calculate(getPrice(inpPrice,open,high,low,close,i),i,rates_total);
      valc[i] = (i>0) ? (val[i]>val[i-1]) ? 1 : (val[i]<val[i-1]) ? 2 : valc[i-1] : 0;
   }
   return(i);
}

//------------------------------------------------------------------
// Custom function(s)
//------------------------------------------------------------------
//
//---
//

class CHull
{
   private :
      int    m_fullPeriod;
      int    m_halfPeriod;
      int    m_sqrtPeriod;
      int    m_arraySize;
      double m_weight1;
      double m_weight2;
      double m_weight3;
      struct sHullArrayStruct
         {
            double value;
            double value3;
            double wsum1;
            double wsum2;
            double wsum3;
            double lsum1;
            double lsum2;
            double lsum3;
         };
      sHullArrayStruct m_array[];

   public :
      CHull() : m_fullPeriod(1), m_halfPeriod(1), m_sqrtPeriod(1), m_arraySize(-1) {                     }
     ~CHull()                                                                      { ArrayFree(m_array); }

      ///
      ///
      ///

      bool init(int period, double divisor)
      {
            m_fullPeriod = (int)(period>1 ? period : 1);
            m_halfPeriod = (int)(m_fullPeriod>1 ? m_fullPeriod/(divisor>1 ? divisor : 1) : 1);
            m_sqrtPeriod = (int) MathSqrt(m_fullPeriod);
            m_arraySize  = -1; m_weight1 = m_weight2 = m_weight3 = 1;
               return(true);
      }

      //
      //
      //

      double calculate( double value, int i, int bars)
      {
         if (m_arraySize<bars) { m_arraySize = ArrayResize(m_array,bars+500); if (m_arraySize<bars) return(0); }

            //
            //
            //

            m_array[i].value=value;
            if (i>m_fullPeriod)
            {
               m_array[i].wsum1 = m_array[i-1].wsum1+value*m_halfPeriod-m_array[i-1].lsum1;
               m_array[i].lsum1 = m_array[i-1].lsum1+value-m_array[i-m_halfPeriod].value;
               m_array[i].wsum2 = m_array[i-1].wsum2+value*m_fullPeriod-m_array[i-1].lsum2;
               m_array[i].lsum2 = m_array[i-1].lsum2+value-m_array[i-m_fullPeriod].value;
            }
            else
            {
               m_array[i].wsum1 = m_array[i].wsum2 =
               m_array[i].lsum1 = m_array[i].lsum2 = m_weight1 = m_weight2 = 0;
               for(int k=0, w1=m_halfPeriod, w2=m_fullPeriod; w2>0 && i>=k; k++, w1--, w2--)
               {
                  if (w1>0)
                  {
                     m_array[i].wsum1 += m_array[i-k].value*w1;
                     m_array[i].lsum1 += m_array[i-k].value;
                     m_weight1        += w1;
                  }
                  m_array[i].wsum2 += m_array[i-k].value*w2;
                  m_array[i].lsum2 += m_array[i-k].value;
                  m_weight2        += w2;
               }
            }
            m_array[i].value3=2.0*m_array[i].wsum1/m_weight1-m_array[i].wsum2/m_weight2;

            //
            //---
            //

            if (i>m_sqrtPeriod)
            {
               m_array[i].wsum3 = m_array[i-1].wsum3+m_array[i].value3*m_sqrtPeriod-m_array[i-1].lsum3;
               m_array[i].lsum3 = m_array[i-1].lsum3+m_array[i].value3-m_array[i-m_sqrtPeriod].value3;
            }
            else
            {
               m_array[i].wsum3 =
               m_array[i].lsum3 = m_weight3 = 0;
               for(int k=0, w3=m_sqrtPeriod; w3>0 && i>=k; k++, w3--)
               {
                  m_array[i].wsum3 += m_array[i-k].value3*w3;
                  m_array[i].lsum3 += m_array[i-k].value3;
                  m_weight3        += w3;
               }
            }
         return(m_array[i].wsum3/m_weight3);
      }
};
CHull iHull;

//
//---
//

template <typename T>
double getPrice(ENUM_APPLIED_PRICE tprice, T& open[], T& high[], T& low[], T& close[], int i)
{
   switch(tprice)
   {
      case PRICE_CLOSE:     return(close[i]);
      case PRICE_OPEN:      return(open[i]);
      case PRICE_HIGH:      return(high[i]);
      case PRICE_LOW:       return(low[i]);
      case PRICE_MEDIAN:    return((high[i]+low[i])/2.0);
      case PRICE_TYPICAL:   return((high[i]+low[i]+close[i])/3.0);
      case PRICE_WEIGHTED:  return((high[i]+low[i]+close[i]+close[i])/4.0);
   }
   return(0);
}
//------------------------------------------------------------------
"#.to_string()
}

// ── BT_UlcerIndex ──

fn gen_mql5_ulcer_index() -> String {
r#"//+------------------------------------------------------------------+
//|                                                 BT_UlcerIndex.mq5 |
//|                        Copyright 2018, MetaQuotes Software Corp. |
//|                                                 https://mql5.com |
//+------------------------------------------------------------------+
#property copyright "Copyright 2018, MetaQuotes Software Corp."
#property link      "https://mql5.com"
#property version   "1.00"
#property description "Ulcer index"
#property indicator_separate_window
#property indicator_buffers 3
#property indicator_plots   1
//--- plot UI
#property indicator_label1  "UI"
#property indicator_type1   DRAW_LINE
#property indicator_color1  clrCrimson
#property indicator_style1  STYLE_SOLID
#property indicator_width1  1

//--- input parameters
input int UIMode =  1;
input int UIPeriod =  24;



//--- indicator buffers
double         BufferUI[];
double         BufferPD[];
double         BufferMA[];
//--- global variables
int            period_ma;
int            handle_ma;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- setting global variables
   period_ma=int(UIPeriod<1 ? 1 : UIPeriod);
//--- indicator buffers mapping
   SetIndexBuffer(0,BufferUI,INDICATOR_DATA);
   SetIndexBuffer(1,BufferPD,INDICATOR_CALCULATIONS);
   SetIndexBuffer(2,BufferMA,INDICATOR_CALCULATIONS);
//--- settings indicators parameters
   IndicatorSetInteger(INDICATOR_DIGITS,Digits());


   IndicatorSetString(INDICATOR_SHORTNAME,"Ulcer index("+(string)UIMode+","+(string)UIPeriod+")");
//--- setting buffer arrays as timeseries
   ArraySetAsSeries(BufferUI,true);
   ArraySetAsSeries(BufferPD,true);
   ArraySetAsSeries(BufferMA,true);
//--- create MA's handles
   ResetLastError();
   handle_ma=iMA(NULL,PERIOD_CURRENT,1,0,MODE_SMA,PRICE_CLOSE);
   if(handle_ma==INVALID_HANDLE)
     {
      Print("The iMA(1) object was not created: Error ",GetLastError());
      return INIT_FAILED;
     }
//---
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
   if(rates_total<period_ma) return 0;
   int limit=rates_total-prev_calculated;
   if(limit>1)
     {
      limit=rates_total-period_ma-1;
      ArrayInitialize(BufferUI,EMPTY_VALUE);
      ArrayInitialize(BufferPD,0);
      ArrayInitialize(BufferMA,0);
     }
   int copied=0,count=(limit==0 ? 1 : rates_total);
   copied=CopyBuffer(handle_ma,0,0,count,BufferMA);
   if(copied!=count) return 0;
   int index;
   double max = 0;
   double Pr = 0;
   for(int i=limit; i>=0; i--)
     {
      if(UIMode == 2)
        {
         index=Lowest(NULL,PERIOD_CURRENT,PRICE_CLOSE,period_ma,i);
         if(index==WRONG_VALUE) return 0;
         max=1/BufferMA[index];
         Pr=1/BufferMA[i];
        }
      else if(UIMode == 1)
        {
         index=Highest(NULL,PERIOD_CURRENT,PRICE_CLOSE,period_ma,i);
         if(index==WRONG_VALUE) return 0;
         max=BufferMA[index];
         Pr=BufferMA[i];
        }

      BufferPD[i]=(pow((Pr-max)/(max!=0 ? max : DBL_MIN),2));




     }
   for(int i=limit; i>=0; i--)
     {
      double MA=MAOnArray(BufferPD,0,period_ma,0,MODE_SMA,i);
      BufferUI[i]=NormalizeDouble((sqrt(MA)*100),4);

     }

//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
int Highest(string symbol_name,const ENUM_TIMEFRAMES timeframe,const ENUM_APPLIED_PRICE price_type,const int count,const int start)
  {
   if(symbol_name=="" || symbol_name==NULL) symbol_name=Symbol();
   double array[];
   int copied=0;
   ArraySetAsSeries(array,true);
   switch(price_type)
     {
      case PRICE_OPEN :
         if(CopyOpen(symbol_name,timeframe,start,count,array)==count)
            return ArrayMaximum(array)+start;
         return WRONG_VALUE;
      case PRICE_HIGH :
         if(CopyHigh(symbol_name,timeframe,start,count,array)==count)
            return ArrayMaximum(array)+start;
         return WRONG_VALUE;
      case PRICE_LOW :
         if(CopyLow(symbol_name,timeframe,start,count,array)==count)
            return ArrayMaximum(array)+start;
         return WRONG_VALUE;
      default:
         if(CopyClose(symbol_name,timeframe,start,count,array)==count)
            return ArrayMaximum(array)+start;
         return WRONG_VALUE;
     }
   return WRONG_VALUE;
  }
//+------------------------------------------------------------------+
int Lowest(string symbol_name,const ENUM_TIMEFRAMES timeframe,const ENUM_APPLIED_PRICE price_type,const int count,const int start)
  {
   if(symbol_name=="" || symbol_name==NULL) symbol_name=Symbol();
   double array[];
   ArraySetAsSeries(array,true);
   switch(price_type)
     {
      case PRICE_OPEN :
         if(CopyOpen(symbol_name,timeframe,start,count,array)==count)
            return ArrayMinimum(array)+start;
         return WRONG_VALUE;
      case PRICE_HIGH :
         if(CopyHigh(symbol_name,timeframe,start,count,array)==count)
            return ArrayMinimum(array)+start;
         return WRONG_VALUE;
      case PRICE_LOW :
         if(CopyLow(symbol_name,timeframe,start,count,array)==count)
            return ArrayMinimum(array)+start;
         return WRONG_VALUE;
      default:
         if(CopyClose(symbol_name,timeframe,start,count,array)==count)
            return ArrayMinimum(array)+start;
         return WRONG_VALUE;
     }
   return WRONG_VALUE;
  }
//+------------------------------------------------------------------+
double MAOnArray(double &array[],int total,int period,int ma_shift,int ma_method,int shift)
  {
   double buf[],arr[];
   if(total==0) total=ArraySize(array);
   if(total>0 && total<=period) return(0);
   if(shift>total-period-ma_shift) return(0);
//---
   switch(ma_method)
     {
      case MODE_SMA :
        {
         total=ArrayCopy(arr,array,0,shift+ma_shift,period);
         if(ArrayResize(buf,total)<0) return(0);
         double sum=0;
         int    i,pos=total-1;
         for(i=1;i<period;i++,pos--)
            sum+=arr[pos];
         while(pos>=0)
           {
            sum+=arr[pos];
            buf[pos]=sum/period;
            sum-=arr[pos+period-1];
            pos--;
           }
         return(buf[0]);
        }
      case MODE_EMA :
        {
         if(ArrayResize(buf,total)<0) return(0);
         double pr=2.0/(period+1);
         int    pos=total-2;
         while(pos>=0)
           {
            if(pos==total-2) buf[pos+1]=array[pos+1];
            buf[pos]=array[pos]*pr+buf[pos+1]*(1-pr);
            pos--;
           }
         return(buf[shift+ma_shift]);
        }
      case MODE_SMMA :
        {
         if(ArrayResize(buf,total)<0) return(0);
         double sum=0;
         int    i,k,pos;
         pos=total-period;
         while(pos>=0)
           {
            if(pos==total-period)
              {
               for(i=0,k=pos;i<period;i++,k++)
                 {
                  sum+=array[k];
                  buf[k]=0;
                 }
              }
            else sum=buf[pos+1]*(period-1)+array[pos];
            buf[pos]=sum/period;
            pos--;
           }
         return(buf[shift+ma_shift]);
        }
      case MODE_LWMA :
        {
         if(ArrayResize(buf,total)<0) return(0);
         double sum=0.0,lsum=0.0;
         double price;
         int    i,weight=0,pos=total-1;
         for(i=1;i<=period;i++,pos--)
           {
            price=array[pos];
            sum+=price*i;
            lsum+=price;
            weight+=i;
           }
         pos++;
         i=pos+period;
         while(pos>=0)
           {
            buf[pos]=sum/weight;
            if(pos==0) break;
            pos--;
            i--;
            price=array[pos];
            sum=sum-lsum+price*period;
            lsum-=array[i];
            lsum+=price;
           }
         return(buf[shift+ma_shift]);
        }
      default: return(0);
     }
   return(0);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_Vortex ──

fn gen_mql5_vortex() -> String {
r#"//+------------------------------------------------------------------+
//|                                                     BT_Vortex.mq5 |
//|                                       copyright "mladen"          |
//|                                       mladenfx@gmail.com          |
//+------------------------------------------------------------------+
#property copyright   "mladen"
#property link        "mladenfx@gmail.com"
#property description "Vortex"
//+------------------------------------------------------------------
#property indicator_separate_window
#property indicator_buffers 9
#property indicator_plots   3
#property indicator_label1  "Filling"
#property indicator_type1   DRAW_FILLING
#property indicator_color1  C'218,231,226',C'255,221,217'
#property indicator_label2  "Vortex +"
#property indicator_type2   DRAW_COLOR_LINE
#property indicator_color2  clrDarkGray,clrDodgerBlue,clrCrimson
#property indicator_width2  2
#property indicator_label3  "Vortex -"
#property indicator_type3   DRAW_COLOR_LINE
#property indicator_color3  clrDarkGray,clrDodgerBlue,clrCrimson
#property indicator_width3  1

//--- input parameters
input int  inpPeriod=32; // Vortex period
//--- buffers declarations
double fillu[],filld[],valp[],valpc[],valm[],valmc[],rngbuffer[],vmpbuffer[],vmmbuffer[];;
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,fillu,INDICATOR_DATA);
   SetIndexBuffer(1,filld,INDICATOR_DATA);
   SetIndexBuffer(2,valp,INDICATOR_DATA);
   SetIndexBuffer(3,valpc,INDICATOR_COLOR_INDEX);
   SetIndexBuffer(4,valm,INDICATOR_DATA);
   SetIndexBuffer(5,valmc,INDICATOR_COLOR_INDEX);
   SetIndexBuffer(6,rngbuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(7,vmpbuffer,INDICATOR_CALCULATIONS);
   SetIndexBuffer(8,vmmbuffer,INDICATOR_CALCULATIONS);
   PlotIndexSetInteger(0,PLOT_SHOW_DATA,false);
//---
   IndicatorSetString(INDICATOR_SHORTNAME,"Vortex ("+(string)inpPeriod+")");
//---
   return (INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator de-initialization function                      |
//+------------------------------------------------------------------+
void OnDeinit(const int reason)
  {
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
int OnCalculate(const int rates_total,const int prev_calculated,const datetime &time[],
                const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[])
  {
   if(Bars(_Symbol,_Period)<rates_total) return(prev_calculated);
   int i=(int)MathMax(prev_calculated-1,1); for(; i<rates_total && !_StopFlag; i++)
     {
      rngbuffer[i] = (i>0) ? MathMax(high[i],close[i-1])-MathMin(low[i],close[i-1]) : high[i]-low[i];
      vmpbuffer[i] = (i>0) ? MathAbs(high[i] - low[i-1]) : MathAbs(high[i] - low[i]);
      vmmbuffer[i] = (i>0) ? MathAbs(low[i] - high[i-1]) : MathAbs(low[i] - high[i]);
      //
      //---
      //
      double vmpSum = 0;
      double vmmSum = 0;
      double rngSum = 0;
      for(int k=0; k<inpPeriod && (i-k)>=0; k++)
        {
         vmpSum += vmpbuffer[i-k];
         vmmSum += vmmbuffer[i-k];
         rngSum += rngbuffer[i-k];
        }
      if(rngSum!=0)
        {
         valp[i] = vmpSum/rngSum;
         valm[i] = vmmSum/rngSum;
        }
      valpc[i] = (valp[i]>valm[i]) ? 1 : 2;
      valmc[i] = (valp[i]>valm[i]) ? 1 : 2;
      fillu[i] = valp[i];
      filld[i] = valm[i];
     }
   return (i);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_Aroon ──

fn gen_mql5_aroon() -> String {
r#"//+------------------------------------------------------------------+
//|                                                     BT_Aroon.mq5 |
//|                             Copyright © 2011,   Nikolay Kositsin |
//|                              Khabarovsk,   farria@mail.redcom.ru  |
//+------------------------------------------------------------------+
//---- author of the indicator
#property copyright "Copyright © 2011, Nikolay Kositsin"
//---- link to the website of the author
#property link "farria@mail.redcom.ru"
//---- Indicator Version Number
#property version   "1.00"
//---- drawing the indicator in a separate window
#property indicator_separate_window
//----two buffers are used for calculation and drawing the indicator
#property indicator_buffers 2
//---- two plots are used
#property indicator_plots   2
//+----------------------------------------------+
//|  Parameters of drawing the bullish indicator |
//+----------------------------------------------+
//---- Drawing indicator 1 as a line
#property indicator_type1   DRAW_LINE
//---- lime color is used as the color of a bullish candlestick
#property indicator_color1  Lime
//---- line of the indicator 1 is a solid curve
#property indicator_style1  STYLE_SOLID
//---- thickness of line of the indicator 1 is equal to 1
#property indicator_width1  1
//---- bullish indicator label display
#property indicator_label1  "BullsAroon"
//+----------------------------------------------+
//|  Parameters of drawing the bearish indicator |
//+----------------------------------------------+
//---- drawing indicator 2 as a line
#property indicator_type2   DRAW_LINE
//---- red color is used as the color of the bearish indicator line
#property indicator_color2  Red
//---- line of the indicator 2 is a solid curve
#property indicator_style2  STYLE_SOLID
//---- thickness of line of the indicator 2 is equal to 1
#property indicator_width2  1
//---- bearish indicator label display
#property indicator_label2  "BearsAroon"
//+----------------------------------------------+
//| Horizontal levels display parameters         |
//+----------------------------------------------+
#property indicator_level1 70.0
#property indicator_level2 50.0
#property indicator_level3 30.0
#property indicator_levelcolor Gray
#property indicator_levelstyle STYLE_DASHDOTDOT
//+----------------------------------------------+
//| Input parameters of the indicator            |
//+----------------------------------------------+
input int AroonPeriod= 9; // period of the indicator
input int AroonShift = 0; // horizontal shift of the indicator in bars
//+----------------------------------------------+
//---- declaration of dynamic arrays that further
// will be used as indicator buffers
double BullsAroonBuffer[];
double BearsAroonBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {

  ArraySetAsSeries(BullsAroonBuffer, false);
  ArraySetAsSeries(BearsAroonBuffer, false);

//---- transformation of the BullsAroonBuffer dynamic indicator into an indicator buffer
   SetIndexBuffer(0,BullsAroonBuffer,INDICATOR_DATA);
//---- shifting the indicator 1 horizontally by AroonShift
   PlotIndexSetInteger(0,PLOT_SHIFT,AroonShift);
//---- performing shift of the beginning of counting of drawing the indicator 1 by AroonPeriod
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,AroonPeriod);
//--- creation of a label to be displayed in the Data Window
   PlotIndexSetString(0,PLOT_LABEL,"BearsAroon");

//---- transformation of the BearsAroonBuffer dynamic array into an indicator buffer
   SetIndexBuffer(1,BearsAroonBuffer,INDICATOR_DATA);
//---- shifting the indicator 2 horizontally by AroonShift
   PlotIndexSetInteger(1,PLOT_SHIFT,AroonShift);
//---- performing shift of the beginning of counting of drawing the indicator 2 by AroonPeriod
   PlotIndexSetInteger(1,PLOT_DRAW_BEGIN,AroonPeriod);
//--- creation of a label to be displayed in the Data Window
   PlotIndexSetString(1,PLOT_LABEL,"BullsAroon");

//---- Initialization of variable for indicator short name
   string shortname;
   StringConcatenate(shortname,"Aroon(",AroonPeriod,", ",AroonShift,")");
//--- creation of the name to be displayed in a separate sub-window and in a pop up help
   IndicatorSetString(INDICATOR_SHORTNAME,shortname);
//--- determination of accuracy of displaying of the indicator values
   IndicatorSetInteger(INDICATOR_DIGITS,0);
//----
  }

//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
int OnCalculate(
                const int rates_total,
                const int prev_calculated,
                const datetime &time[],
                const double &open[],
                const double& high[],
                const double& low[],
                const double &close[],
                const long &tick_volume[],
                const long &volume[],
                const int &spread[]
                )
  {

  ArraySetAsSeries(high, false);
  ArraySetAsSeries(low, false);

//---- checking the number of bars to be enough for the calculation
   if(rates_total<AroonPeriod-1)
      return(0);

//---- declaration of local variables
   int first,bar;
   double BULLS,BEARS;

//---- calculation of the starting number 'first' for the cycle of recalculation of bars
   if(prev_calculated>rates_total || prev_calculated<=0)
      first=AroonPeriod-1;

   else first=prev_calculated-1;

//---- main cycle of calculation of the indicator
   for(bar=first; bar<rates_total; bar++)
     {
      //---- calculation of the indicator values
      BULLS = 100 - getHighestIndex(high, bar) * 100.0 / AroonPeriod;
      BEARS = 100 - getLowestIndex(low, bar) * 100.0 / AroonPeriod;

      //---- initialization of cells of the indicator buffers with obtained values
      BullsAroonBuffer[bar] = BULLS;
      BearsAroonBuffer[bar] = BEARS;
     }
//----
   return(rates_total);
  }
//+------------------------------------------------------------------+

int getHighestIndex(const double &high[], int startIndex){
   double highestValue = -1;
   int highestIndex = 0;

   for(int a=startIndex-AroonPeriod+1; a<=startIndex; a++){
      double value = high[a];

      if(value > highestValue){
         highestIndex = startIndex - a;
         highestValue = value;
      }
   }

   return highestIndex;
}

int getLowestIndex(const double &low[], int startIndex){
   double lowestValue = 10000000;
   int lowestIndex = 0;

   for(int a=startIndex-AroonPeriod+1; a<=startIndex; a++){
      double value = low[a];

      if(value < lowestValue){
         lowestIndex = startIndex - a;
         lowestValue = value;
      }
   }

   return lowestIndex;
}
"#.to_string()
}

// ── BT_HighestInRange ──

fn gen_mql5_highest_in_range() -> String {
r#"//+------------------------------------------------------------------+
//|                                             BT_HighestInRange.mq5 |
//|                                    Copyright 2019, StrategyQuant |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "2019, StrategyQuant"
#property link        "http://www.strategyquant.com"
#property description "SqHighestInRange"

#property indicator_buffers 1
#property indicator_plots 1
#property indicator_label1  "Highest in range"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Cyan
#property indicator_chart_window

#define DAY_SECONDS 24 * 60 * 60

//--- input parameters
input string TimeFrom="00:00";
input string TimeTo="00:00";
//---- buffers
double ExtBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+

datetime nextStartTime, nextEndTime;
double lastValue = 0;
double lastUsableValue = 0;
double highestValue = 0;

int OnInit()
  {
//--- check for input parameters
   if(StringFind(TimeFrom, ":") < 0){
      printf("Incorrect value for input variable TimeFrom. Time must be in format HH:MM", TimeFrom);
      return(INIT_FAILED);
   }

   if(StringFind(TimeTo, ":") < 0){
      printf("Incorrect value for input variable TimeTo. Time must be in format HH:MM", TimeTo);
      return(INIT_FAILED);
   }

//---- indicator buffers
   ArraySetAsSeries(ExtBuffer, false);
   SetIndexBuffer(0,ExtBuffer);
//--- indicator short name
   string short_name="HighestInRange(" + TimeFrom + "-" + TimeTo + ")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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

  ArraySetAsSeries(time, false);
  ArraySetAsSeries(open, false);
  ArraySetAsSeries(high, false);
  ArraySetAsSeries(low, false);
  ArraySetAsSeries(close, false);
  ArraySetAsSeries(tick_volume, false);
  ArraySetAsSeries(volume, false);
  ArraySetAsSeries(spread, false);

//--- detect start position
   int startIndex;

   if(prev_calculated > 1) startIndex = prev_calculated - 1;
   else {
      MqlDateTime curTime;
      if(!TimeToStruct(time[0], curTime)){
         Alert("SqHighestInRange indicator error - Cannot load current time");
      }

      nextStartTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeFrom));
      nextEndTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeTo));

      if(nextEndTime < nextStartTime){
         nextEndTime += DAY_SECONDS;
      }

      startIndex=1;
      ExtBuffer[0]=0.0;
   }

//--- main cycle
   for(int i=startIndex; i<rates_total && !IsStopped(); i++){
      if(time[i] >= nextEndTime){
         MqlDateTime curTime;
         if(!TimeToStruct(time[i], curTime)){
            Alert("SqHighestInRange indicator error - Cannot load current time");
         }

         nextStartTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeFrom));
         nextEndTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeTo));

         lastValue = highestValue;
         highestValue = 0;

         if(nextEndTime <= time[i]){
            if(nextEndTime < nextStartTime){
               nextEndTime += DAY_SECONDS;
            }
            else {
               nextStartTime += DAY_SECONDS;
               nextEndTime += DAY_SECONDS;
            }
         }

         if(nextStartTime <= time[i]){
            highestValue = high[i];
         }
      }
      else if(time[i] >= nextStartTime){
         highestValue = MathMax(highestValue, high[i]);
      }
      else {
         highestValue = 0;
      }

      if(lastValue > 0){
         lastUsableValue = lastValue;
         ExtBuffer[i] = lastValue;
      }
      else {
         ExtBuffer[i] = lastUsableValue;
      }
   }

//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_LowestInRange ──

fn gen_mql5_lowest_in_range() -> String {
r#"//+------------------------------------------------------------------+
//|                                              BT_LowestInRange.mq5 |
//|                                    Copyright 2019, StrategyQuant |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "2019, StrategyQuant"
#property link        "http://www.strategyquant.com"
#property description "SqLowestInRange"

#property indicator_buffers 1
#property indicator_plots 1
#property indicator_label1  "Lowest in range"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Yellow
#property indicator_chart_window

#define INF 0x6FFFFFFF
#define DAY_SECONDS 24 * 60 * 60

//--- input parameters
input string TimeFrom="00:00";
input string TimeTo="00:00";
//---- buffers
double ExtBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+

datetime nextStartTime, nextEndTime;
double lastValue = 0;
double lastUsableValue = 0;
double lowestValue = 0;

int OnInit()
  {
//--- check for input parameters
   if(StringFind(TimeFrom, ":") < 0){
      printf("Incorrect value for input variable TimeFrom. Time must be in format HH:MM", TimeFrom);
      return(INIT_FAILED);
   }

   if(StringFind(TimeTo, ":") < 0){
      printf("Incorrect value for input variable TimeTo. Time must be in format HH:MM", TimeTo);
      return(INIT_FAILED);
   }

//---- indicator buffers
   SetIndexBuffer(0,ExtBuffer);
   ArraySetAsSeries(ExtBuffer, false);
//--- indicator short name
   string short_name="LowestInRange(" + TimeFrom + "-" + TimeTo + ")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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

  ArraySetAsSeries(time, false);
  ArraySetAsSeries(open, false);
  ArraySetAsSeries(high, false);
  ArraySetAsSeries(low, false);
  ArraySetAsSeries(close, false);
  ArraySetAsSeries(tick_volume, false);
  ArraySetAsSeries(volume, false);
  ArraySetAsSeries(spread, false);

//--- detect start position
   int startIndex;

   if(prev_calculated > 1) startIndex = prev_calculated - 1;
   else {
      MqlDateTime curTime;
      if(!TimeToStruct(time[0], curTime)){
         Alert("SqLowestInRange indicator error - Cannot load current time");
      }

      nextStartTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeFrom));
      nextEndTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeTo));

      if(nextEndTime < nextStartTime){
         nextEndTime += DAY_SECONDS;
      }

      startIndex=1;
      ExtBuffer[0]=0.0;
   }

//--- main cycle
   for(int i=startIndex; i<rates_total && !IsStopped(); i++){
      if(time[i] >= nextEndTime){
         MqlDateTime curTime;
         if(!TimeToStruct(time[i], curTime)){
            Alert("SqLowestInRange indicator error - Cannot load current time");
         }

         nextStartTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeFrom));
         nextEndTime = StringToTime(StringFormat("%04d.%02d.%02d %s", curTime.year, curTime.mon, curTime.day, TimeTo));

         lastValue = lowestValue;
         lowestValue = INF;

         if(nextEndTime <= time[i]){
            if(nextEndTime < nextStartTime){
               nextEndTime += DAY_SECONDS;
            }
            else {
               nextStartTime += DAY_SECONDS;
               nextEndTime += DAY_SECONDS;
            }
         }

         if(nextStartTime <= time[i]){
            lowestValue = low[i];
         }
      }
      else if(time[i] >= nextStartTime){
         lowestValue = MathMin(lowestValue, low[i]);
      }
      else {
         lowestValue = INF;
      }

      if(lastValue < INF){
         lastUsableValue = lastValue;
         ExtBuffer[i] = lastValue;
      }
      else {
         ExtBuffer[i] = lastUsableValue;
      }
   }

//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_Reflex ──

fn gen_mql5_reflex() -> String {
r#"//+------------------------------------------------------------------+
//|                                                     BT_Reflex.mq5 |
//|                  copyright "c mladen, 2020 corrected by Clonex SQX"|
//|                  mladenfx@gmail.com                                |
//+------------------------------------------------------------------+
#property copyright   "c mladen, 2020 corrected by Clonex SQX"
#property link        "mladenfx@gmail.com"
#property version     "1.1"
//------------------------------------------------------------------
#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   1
#property indicator_label1  "Reflex"
#property indicator_type1   DRAW_COLOR_LINE
#property indicator_color1  clrDodgerBlue,clrCoral
#property indicator_width1  2

//
//---
//

input int inpReflexPeriod = 24; // Reflex period
double  val[],valc[];

//------------------------------------------------------------------
//
//------------------------------------------------------------------
//
//
//

int OnInit()
{
   SetIndexBuffer(0,val   ,INDICATOR_DATA);
   SetIndexBuffer(1,valc  ,INDICATOR_COLOR_INDEX);

      iReflex.OnInit(inpReflexPeriod);

   //
   //
   //

   IndicatorSetString(INDICATOR_SHORTNAME,"Reflex ("+(string)inpReflexPeriod+")");
   return(INIT_SUCCEEDED);
}

//------------------------------------------------------------------
//
//------------------------------------------------------------------
//
//
//
//

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
   int limit = MathMax(prev_calculated-1,0);
   for(int i=limit; i<rates_total && !_StopFlag; i++)
   {
      val[i]  = iReflex.OnCalculate(close[i],i,rates_total);
      valc[i] = (i>0) ? val[i]>val[i-1] ? 0 :  val[i]<val[i-1] ? 1 : 0 : 0;
   }
   return(rates_total);
}


//------------------------------------------------------------------
//
//------------------------------------------------------------------
//
//
//

class CReflex
{
   private :
         double m_c1;
         double m_c2;
         double m_c3;
         double m_multi;
         double test;
         double  cosine(double a){    return MathCos(a * M_PI/180.0);}
         struct sWorkStruct
         {
            double value;
            double ssm;
            double sum;
            double ms;
         };
         sWorkStruct m_array[];
         int         m_arraySize;
         int         m_period;

   public :
      CReflex() : m_c1(1), m_c2(1), m_c3(1), m_arraySize(-1) {  }
     ~CReflex()                                              {  }

      //
      //---
      //

      void OnInit(int period)
      {

         m_period = (period>1) ? period : 1;


         //// TS varianta originalna
         double a1 = MathExp(-1.414*M_PI/(double)(m_period*0.5));
         double b1 = 2.0*a1*cosine(MathCos((1.414*180)/(m_period*0.5)));

            test =  b1;
            m_c2 = b1;
            m_c3 = -a1*a1;
            m_c1 = 1.0 - m_c2 - m_c3;

            //
            //
            //

            m_multi = 1; for (int k=1; k<m_period; k++) m_multi += (k+1);
      }
      double OnCalculate(double value, int i, int bars)
      {
         if (m_arraySize<bars) m_arraySize=ArrayResize(m_array,bars+500);


         //
         //
         //

         m_array[i].value = value;
            if (i>1)
                    m_array[i].ssm = m_c1*(m_array[i].value+m_array[i-1].value)/2.0 + m_c2*m_array[i-1].ssm + m_c3*m_array[i-2].ssm;
            else    m_array[i].ssm = value;
            if (i>m_period)
                  m_array[i].sum = m_array[i-1].sum + m_array[i].ssm - m_array[i-m_period].ssm;
            else
               {
                  m_array[i].sum = m_array[i].ssm;
                     for (int k=1; k<m_period && (i-k)>=0; k++) m_array[i].sum += m_array[i-k].ssm;
               }

               //
               //
               //



               double tslope = (i>=m_period) ? (m_array[i-m_period].ssm - m_array[i].ssm)/m_period : 0;

                double sum = 0;
                if (i>inpReflexPeriod){

                 for(int a=1; a<=inpReflexPeriod; a++){
                 sum = sum+ m_array[i].ssm+a*tslope- m_array[i-a].ssm;
                 }

               }
               sum = sum/inpReflexPeriod;

               m_array[i].ms = (i>0) ? 0.04 * sum*sum+0.96*m_array[i-1].ms : 0;
       return (m_array[i].ms!=0 ? sum/MathSqrt(m_array[i].ms) : 0);
      }
};
CReflex iReflex;
"#.to_string()
}

// ── BT_AvgVolume ──

fn gen_mql5_avg_volume() -> String {
r#"//+------------------------------------------------------------------+
//|                                                  BT_AvgVolume.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright "Copyright © 2017, StrategyQuant s.r.o."
#property link      "http://www.strategyquant.com"
#property version   "1.00"
#property indicator_separate_window
#property indicator_buffers 2
#property indicator_plots   2
#property indicator_type1   DRAW_LINE
#property indicator_type2   DRAW_HISTOGRAM
#property indicator_color1 Red
#property indicator_color2 White

#property indicator_width2 3

input int MAPeriod = 14;

double vol[];
double avgVol[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+
void OnInit()
  {
   SetIndexBuffer(0,avgVol);
   PlotIndexSetString(0,PLOT_LABEL,"Average Volume("+IntegerToString(MAPeriod)+")");

   SetIndexBuffer(1,vol);
   PlotIndexSetString(1,PLOT_LABEL,"");

   IndicatorSetString(INDICATOR_SHORTNAME,"AvgVolume");
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//---

   if(rates_total < MAPeriod) {
      int start = prev_calculated > 0 ? prev_calculated - 1 : 0;

      for(int i=start; i<rates_total; i++){
         vol[i] = (double) (volume[i] ? volume[i] : tick_volume[i]);
         avgVol[i] = 0;
      }
      return(rates_total);
   }

   double tempv;
   int limit = prev_calculated < MAPeriod ? MAPeriod - 1 : prev_calculated - 1;

   for(int i=limit; i<rates_total; i++) {
      vol[i] = (double) (volume[i] ? volume[i] : tick_volume[i]);

      tempv = 0;
      for (int n=i-MAPeriod+1;n<=i;n++) {
         tempv += vol[n];
      }

      avgVol[i] = tempv / MAPeriod;
   }

//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_BBWidthRatio ──

fn gen_mql5_bb_width_ratio() -> String {
r#"//+------------------------------------------------------------------+
//|                                               BT_BBWidthRatio.mq5 |
//|                           Copyright © @2017 StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property  copyright "Copyright © @2017 StrategyQuant s.r.o."
#property  link      "http://www.strategyquant.com"

#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "Bollinger Bands width ratio"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Blue

//---- indicator parameters
input int    BandsPeriod=20;
input double BandsDeviations=2.0;
input int    AppliedPrice=PRICE_CLOSE;

//---- buffers
double ind_buffer[];

int period, mode;
int stdDevHandle, smaHandle;

void OnInit()
  {
   if(BandsPeriod <= 0){
      printf("Incorrect value for input variable BandsPeriod=%d. Indicator will use value=%d for calculations.", BandsPeriod, 14);
      period = 14;
   }
   else period = BandsPeriod;

   switch(AppliedPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = AppliedPrice;
         break;
      default:
         printf("Incorrect value for input variable AppliedPrice=%d. Indicator will use value PRICE_CLOSE for calculations.", AppliedPrice);
         mode = PRICE_CLOSE;
   }

   ArraySetAsSeries(ind_buffer, true);

   SetIndexBuffer(0, ind_buffer);

   stdDevHandle = iStdDev(NULL, 0, period, 0, MODE_SMA, mode);
   smaHandle = iMA(NULL, 0, period, 0, MODE_SMA, mode);

//--- indicator short name
   string short_name="BBWidthRatio("+string(period)+","+string(BandsDeviations)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
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

   if(rates_total < period) return(0);

   int limit;

   if(prev_calculated > 0) limit = rates_total - prev_calculated + 1;
   else {
      for(int a=0; a<rates_total; a++){
         ind_buffer[a] = 0.0;
      }

      limit = rates_total - period;
   }

   for(int i=limit-1; i>=0; i--) {
      double stdDev = getIndicatorValue(stdDevHandle, 0, i);
      double sma = getIndicatorValue(smaHandle, 0, i);

      if(sma == 0){
         ind_buffer[i] = 0;
      }
      else {
         ind_buffer[i] = 2.0 * BandsDeviations * stdDev / sma;
      }
   }
   return(rates_total);
  }
//+------------------------------------------------------------------+

double getIndicatorValue(int indyHandle, int bufferIndex, int shift){
   double buffer[];

   if(CopyBuffer(indyHandle, bufferIndex, shift, 1, buffer) < 0) {
      PrintFormat("Failed to copy data from the indicator, error code %d", GetLastError());
      return(0);
   }

   double val = buffer[0];
   return (val >= -1000 && val <= 1000) ? val : 0;
}
"#.to_string()
}

// ── BT_EfficiencyRatio ──

fn gen_mql5_efficiency_ratio() -> String {
r#"//+--------------------------------------------------------------------------------------------------+
//|                                                              BT_EfficiencyRatio.mq5 |
//|                                                                    Copyright c 2011, barmenteros |
//|                                                            http://www.mql4.com/users/barmenteros |
//+--------------------------------------------------------------------------------------------------+
#property copyright     "Copyright c 2011, barmenteros"
#property link          "barmenteros.fx@gmail.com"
#property version       "1.00"
#property description   "Kaufman Efficiency Ratio (also called \"generalized fractal"
#property description   "efficiency\") according to Perry Kaufman books \"Smarter Trading\""
#property description   "and \"Trading Systems & Methods\"."
//--- indicator settings
#property indicator_separate_window
#property indicator_buffers 1
#property indicator_plots   1
#property indicator_minimum 0.0
#property indicator_maximum 1.0
#property indicator_color1  clrRed
#property indicator_label1  "Kaufman Efficiency Ratio"
//+--------------------------------------------------------------------------------------------------+
//| Enumerations                                                                                     |
//+--------------------------------------------------------------------------------------------------+
enum his_switch
  {
   On,
   Off
  };
//--- input parameters
input uchar       ERperiod=10;            // Efficiency ratio period
his_switch  histogram=Off;          // Histogram switch
char        shift=0;                // Horizontal shift (in bars)
//--- indicator buffers
double         ERBfr[];
//+--------------------------------------------------------------------------------------------------+
//| NetPriceMovement                                                                                 |
//+--------------------------------------------------------------------------------------------------+
double NetPriceMovement(int initialbar, int period, const double &price[])
   {
    double n;
    n=MathAbs(price[initialbar]-price[initialbar-period]);
    return(n);
   }
//+--------------------------------------------------------------------------------------------------+
//| Volatility                                                                                       |
//+--------------------------------------------------------------------------------------------------+
double Volatility(int initialbar, int period, const double &price[])
   {
    int j;
    double v=0.0;
    for(j=0; j<period; j++)
      v+=MathAbs(price[initialbar-j]-price[initialbar-1-j]);
    return(v);
   }
//+--------------------------------------------------------------------------------------------------+
//| Custom indicator initialization function                                                         |
//+--------------------------------------------------------------------------------------------------+
void OnInit()
  {
//--- indicator buffers mapping
   SetIndexBuffer(0,ERBfr);
//--- set accuracy
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits);
// ---- drawing settings
   if(histogram==Off) PlotIndexSetInteger(0,PLOT_DRAW_TYPE,DRAW_LINE);
   else               PlotIndexSetInteger(0,PLOT_DRAW_TYPE,DRAW_HISTOGRAM);
//---- line shifts when drawing
   PlotIndexSetInteger(0,PLOT_SHIFT,shift);
//--- name for DataWindow and indicator subwindow label
   string short_name="KEffRatio(";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name+string(ERperiod)+")");
   PlotIndexSetString(0,PLOT_LABEL,short_name+string(ERperiod)+")");
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ERperiod);
//--- initialization done
  }
//+--------------------------------------------------------------------------------------------------+
//| Custom indicator iteration function                                                              |
//+--------------------------------------------------------------------------------------------------+
int OnCalculate(const int rates_total,    // size of the price[] array
                const int prev_calculated,// bars handled on a previous call
                const int begin,          // where the significant data start from
                const double &price[])    // array to calculate
  {
//--- declaration of local variables
   int    limit,i;
   double direction,noise;
//--- check for bars count
   if(rates_total<ERperiod-1+begin)
      return(0); // not enough bars for calculation
//--- first calculation or number of bars was changed
   if(prev_calculated==0)// first calculation
      {
       limit=ERperiod+begin;
       ArrayInitialize(ERBfr,EMPTY_VALUE);
      }
   else limit=prev_calculated-1;
//--- main loop
   for(i=limit;i<rates_total;i++)
      {
       direction=NetPriceMovement(i, ERperiod, price);
       noise=Volatility(i, ERperiod, price);
       if(noise==0.0) noise=0.000000001;
       ERBfr[i]=direction/noise;
      }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+--------------------------------------------------------------------------------------------------+
"#.to_string()
}

// ── BT_HighestIndex ──

fn gen_mql5_highest_index() -> String {
r#"//+------------------------------------------------------------------+
//|                                               BT_HighestIndex.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "SqHighestIndex"

#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "HighestIndex"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Red

//--- input parameters
input int InpPeriod=14; // Period
input int InpPrice=2;
//---- buffers
double    ExtBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+

int period, mode;

void OnInit()
  {
//--- check for input parameters
   if(InpPeriod<=0)
     {
      printf("Incorrect value for input variable InpPeriod=%d. Indicator will use value=%d for calculations.",InpPeriod,14);
      period=14;
     }
   else period = InpPeriod;

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_HIGH for calculations.",InpPrice);
         mode=PRICE_HIGH;
   }

//---- indicator buffers
   SetIndexBuffer(0,ExtBuffer);
//--- indicator short name
   string short_name="HighestIndex("+string(period)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- checking for bars count
   if(rates_total<period)
      return(0);
//--- detect start position
   int start;
   if(prev_calculated>1) start=prev_calculated-1;
   else
     {
      start=1;
      ExtBuffer[0]=0.0;
     }

//--- main cycle
   for(int i=start;i<rates_total && !IsStopped();i++)
   {
      if(i < period - 1){
         ExtBuffer[i] = 0.0;
      }
      else {
         int highestIndex = 0;
         double highestValue = -1;

         for(int a=i-period + 1; a<=i; a++){
            double value = getValue(open, high, low, close, mode, a);

            if(value - highestValue > 0.00000001){
               highestIndex = i - a;
               highestValue = value;
            }
         }

         ExtBuffer[i] = highestIndex;
      }
   }
//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+

double getValue(const double &open[],
                const double &high[],
                const double &low[],
                const double &close[],
                int priceMode,
                int index)
{
   switch(priceMode){
      case PRICE_OPEN: return open[index];
      case PRICE_HIGH: return high[index];
      case PRICE_LOW: return low[index];
      case PRICE_CLOSE: return close[index];
      case PRICE_MEDIAN: return (high[index] + low[index]) / 2;
      case PRICE_TYPICAL: return (high[index] + low[index] + close[index]) / 3;
      case PRICE_WEIGHTED: return (high[index] + low[index] + close[index] + close[index]) / 4;
      default: return 0;
   }
}
"#.to_string()
}

// ── BT_KAMA ──

fn gen_mql5_kama() -> String {
r#"//+------------------------------------------------------------------+
//|                                                       BT_KAMA.mq5 |
//|                        Copyright 2009, MetaQuotes Software Corp. |
//|                                              http://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright   "2009, MetaQuotes Software Corp."
#property link        "http://www.mql5.com"
#property version     "1.00"
#property description "Kaufmans Adaptive Moving Average"

#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots   1
//---- plot ExtAMABuffer
#property indicator_label1  "KAMA"
#property indicator_type1   DRAW_LINE
#property indicator_color1  Red
#property indicator_style1  STYLE_SOLID
#property indicator_width1  1
//--- default applied price
#property indicator_applied_price PRICE_OPEN
//--- input parameters
input int      InpPeriodAMA=10;      // AMA period
input int      InpFastPeriodEMA=2;   // Fast EMA period
input int      InpSlowPeriodEMA=30;  // Slow EMA period
int      InpShiftAMA=0;        // AMA shift
//--- indicator buffers
double         ExtAMABuffer[];
//--- global variables
double         ExtFastSC;
double         ExtSlowSC;
int            ExtPeriodAMA;
int            ExtSlowPeriodEMA;
int            ExtFastPeriodEMA;
//+------------------------------------------------------------------+
//| AMA initialization function                                      |
//+------------------------------------------------------------------+
int OnInit()
  {
//--- check for input values
   if(InpPeriodAMA<=0)
     {
      ExtPeriodAMA=10;
      printf("Input parameter InpPeriodAMA has incorrect value (%d). Indicator will use value %d for calculations.",
             InpPeriodAMA,ExtPeriodAMA);
     }
   else ExtPeriodAMA=InpPeriodAMA;
   if(InpSlowPeriodEMA<=0)
     {
      ExtSlowPeriodEMA=30;
      printf("Input parameter InpSlowPeriodEMA has incorrect value (%d). Indicator will use value %d for calculations.",
             InpSlowPeriodEMA,ExtSlowPeriodEMA);
     }
   else ExtSlowPeriodEMA=InpSlowPeriodEMA;
   if(InpFastPeriodEMA<=0)
     {
      ExtFastPeriodEMA=2;
      printf("Input parameter InpFastPeriodEMA has incorrect value (%d). Indicator will use value %d for calculations.",
             InpFastPeriodEMA,ExtFastPeriodEMA);
     }
   else ExtFastPeriodEMA=InpFastPeriodEMA;
//--- indicator buffers mapping
   SetIndexBuffer(0,ExtAMABuffer,INDICATOR_DATA);
//--- set shortname and change label
   string short_name="KAMA("+IntegerToString(ExtPeriodAMA)+","+
                      IntegerToString(ExtFastPeriodEMA)+","+
                      IntegerToString(ExtSlowPeriodEMA)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
   PlotIndexSetString(0,PLOT_LABEL,short_name);
//--- set accuracy
   IndicatorSetInteger(INDICATOR_DIGITS,_Digits+1);
//--- sets first bar from what index will be drawn
   PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ExtPeriodAMA);
//--- set index shift
   PlotIndexSetInteger(0,PLOT_SHIFT,InpShiftAMA);
//--- calculate ExtFastSC & ExtSlowSC
   ExtFastSC=2.0/(ExtFastPeriodEMA+1.0);
   ExtSlowSC=2.0/(ExtSlowPeriodEMA+1.0);
//--- OnInit done
   return(INIT_SUCCEEDED);
  }
//+------------------------------------------------------------------+
//| AMA iteration function                                           |
//+------------------------------------------------------------------+
int OnCalculate(const int rates_total,
                const int prev_calculated,
                const int begin,
                const double &price[])
  {
   int i;
//--- check for rates count
   if(rates_total<ExtPeriodAMA+begin)
      return(0);
//--- draw begin may be corrected
   if(begin!=0) PlotIndexSetInteger(0,PLOT_DRAW_BEGIN,ExtPeriodAMA+begin);
//--- detect position
   int pos=prev_calculated-1;
//--- first calculations
   if(pos<ExtPeriodAMA+begin)
     {
      pos=ExtPeriodAMA+begin;
      for(i=0;i<pos-1;i++) ExtAMABuffer[i]=0.0;
      ExtAMABuffer[pos-1]=price[pos-1];
     }
//--- main cycle
   for(i=pos;i<rates_total && !IsStopped();i++)
     {
      //--- calculate SSC
      double dCurrentSSC=(CalculateER(i,price)*(ExtFastSC-ExtSlowSC))+ExtSlowSC;
      //--- calculate AMA
      double dPrevAMA=ExtAMABuffer[i-1];
      ExtAMABuffer[i]=pow(dCurrentSSC,2)*(price[i]-dPrevAMA)+dPrevAMA;
     }
//--- return value of prev_calculated for next call
   return(rates_total);
  }
//+------------------------------------------------------------------+
//| Calculate ER value                                               |
//+------------------------------------------------------------------+
double CalculateER(const int nPosition,const double &PriceData[])
  {
   double dSignal=fabs(PriceData[nPosition]-PriceData[nPosition-ExtPeriodAMA]);
   double dNoise=0.0;
   for(int delta=0;delta<ExtPeriodAMA;delta++)
      dNoise+=fabs(PriceData[nPosition-delta]-PriceData[nPosition-delta-1]);
   if(dNoise!=0.0)
      return(dSignal/dNoise);
   return(0.0);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_LowestIndex ──

fn gen_mql5_lowest_index() -> String {
r#"//+------------------------------------------------------------------+
//|                                                BT_LowestIndex.mq5 |
//|                           Copyright © 2017, StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright   "Copyright © 2017, StrategyQuant s.r.o."
#property link        "http://www.strategyquant.com"
#property description "SqLowestIndex"

#property indicator_chart_window
#property indicator_buffers 1
#property indicator_plots 1

#property indicator_label1  "LowestIndex"
#property indicator_type1  DRAW_LINE
#property indicator_color1 Red

//--- input parameters
input int InpPeriod=14; // Period
input int InpPrice=2;
//---- buffers
double    ExtBuffer[];
//+------------------------------------------------------------------+
//| Custom indicator initialization function                         |
//+------------------------------------------------------------------+

int period, mode;

void OnInit()
  {
//--- check for input parameters
   if(InpPeriod<=0)
     {
      printf("Incorrect value for input variable InpPeriod=%d. Indicator will use value=%d for calculations.",InpPeriod,14);
      period=14;
     }
   else period = InpPeriod;

   switch(InpPrice){
      case PRICE_OPEN:
      case PRICE_HIGH:
      case PRICE_LOW:
      case PRICE_CLOSE:
      case PRICE_MEDIAN:
      case PRICE_TYPICAL:
      case PRICE_WEIGHTED:
         mode = InpPrice;
         break;
      default:
         printf("Incorrect value for input variable InpPrice=%d. Indicator will use value PRICE_LOW for calculations.",InpPrice);
         mode=PRICE_LOW;
   }

//---- indicator buffers
   SetIndexBuffer(0,ExtBuffer);
//--- indicator short name
   string short_name="LowestIndex("+string(period)+")";
   IndicatorSetString(INDICATOR_SHORTNAME,short_name);
//---- end of initialization function
  }
//+------------------------------------------------------------------+
//| Custom indicator iteration function                              |
//+------------------------------------------------------------------+
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
//--- checking for bars count
   if(rates_total<period)
      return(0);
//--- detect start position
   int start;
   if(prev_calculated>1) start=prev_calculated-1;
   else
     {
      start=1;
      ExtBuffer[0]=0.0;
     }

//--- main cycle
   for(int i=start;i<rates_total && !IsStopped();i++)
   {
      if(i < period - 1){
         ExtBuffer[i] = 0.0;
      }
      else {
         int lowestIndex = 0;
         double lowestValue = 10000000;

         for(int a=i-period+1; a<=i; a++){
            double value = 0;

            switch(mode){
               case PRICE_OPEN:
                  value = open[a];
                  break;
               case PRICE_HIGH:
                  value = high[a];
                  break;
               case PRICE_LOW:
                  value = low[a];
                  break;
               case PRICE_CLOSE:
                  value = close[a];
                  break;
               case PRICE_MEDIAN:
                  value = (high[a] + low[a]) / 2;
                  break;
               case PRICE_TYPICAL:
                  value = (high[a] + low[a] + close[a]) / 3;
                  break;
               case PRICE_WEIGHTED:
                  value = (high[a] + low[a] + close[a] + close[a]) / 4;
                  break;
            }

            if(lowestValue - value > 0.00000001){
               lowestIndex = i - a;
               lowestValue = value;
            }
         }

         ExtBuffer[i] = lowestIndex;
      }
   }
//---- OnCalculate done. Return new prev_calculated.
   return(rates_total);
  }
//+------------------------------------------------------------------+
"#.to_string()
}

// ── BT_QQE ──

fn gen_mql5_qqe() -> String {
r#"//+------------------------------------------------------------------+
//|   Qualitative Quantitative Estimation Indicator for Metatrader 5 |
//|                            Copyright © 2017 StrategyQuant s.r.o. |
//|                                     http://www.strategyquant.com |
//+------------------------------------------------------------------+
#property copyright "Copyright © 2017 StrategyQuant s.r.o."
#property link      "http://www.strategyquant.com"

#property indicator_separate_window
#property indicator_plots 6
#property indicator_buffers 6

#property indicator_label1 "Value 1"
#property indicator_color1 Navy
#property indicator_style1 STYLE_SOLID
#property indicator_type1 DRAW_LINE
#property indicator_width1 2

#property indicator_label2 "Value 2"
#property indicator_color2 Red
#property indicator_style2 STYLE_DOT
#property indicator_type2 DRAW_LINE

input int RSI_Period = 14;
input int SF = 5;
input double WF = 4.236;

int Wilders_Period;
int StartBar;

int rsiHandle;

double TrLevelSlow[];
double AtrRsi[];
double MaAtrRsi[];
double Rsi[];
double RsiMa[];
double MaAtrRsiWP[];

void OnInit()
{
    Wilders_Period = RSI_Period * 2 - 1;
    if (Wilders_Period < SF)
        StartBar = SF;
    else
        StartBar = Wilders_Period;

    ArraySetAsSeries(RsiMa, true);
    ArraySetAsSeries(TrLevelSlow, true);
    ArraySetAsSeries(Rsi, true);
    ArraySetAsSeries(AtrRsi, true);
    ArraySetAsSeries(MaAtrRsi, true);
    ArraySetAsSeries(MaAtrRsiWP, true);

    SetIndexBuffer(0, RsiMa);
    SetIndexBuffer(1, TrLevelSlow);
    SetIndexBuffer(2, Rsi);
    SetIndexBuffer(3, AtrRsi);
    SetIndexBuffer(4, MaAtrRsi);
    SetIndexBuffer(5, MaAtrRsiWP);

    rsiHandle = iRSI(NULL, 0, RSI_Period, PRICE_CLOSE);

    IndicatorSetString(INDICATOR_SHORTNAME, "QQE(" + IntegerToString(SF) + ")");
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
    int counted, i;
    double rsi0, rsi1, dar, tr, dv;

    if(rates_total <= StartBar) return(0);

    if(prev_calculated == 0){
        for(i = rates_total - 1; i >= 0; i--){
            RsiMa[i] = 0.0;
            TrLevelSlow[i] = 0.0;
            Rsi[i] = 0.0;
            AtrRsi[i] = 0.0;
            MaAtrRsi[i] = 0.0;
            MaAtrRsiWP[i] = 0.0;
        }
    }

    counted = rates_total - (prev_calculated == 0 ? 2 : prev_calculated);

    for (i = counted; i >= 0; i--){
        Rsi[i] = getIndicatorValue(rsiHandle, 0, i);
        RsiMa[i] = Rsi[i];
    }

    for (i = counted; i >= 0; i--){
        RsiMa[i] = RsiMa[i] * (2.0 / (1 + SF)) + (1 - (2.0 / (1 + SF))) * RsiMa[i + 1];
        AtrRsi[i] = MathAbs(RsiMa[i + 1] - RsiMa[i]);
        MaAtrRsi[i] = AtrRsi[i];
    }

    for (i = counted; i >= 0; i--){
        MaAtrRsi[i] = MaAtrRsi[i] * (2.0 / (1 + Wilders_Period)) + (1 - (2.0 / (1 + Wilders_Period))) * MaAtrRsi[i + 1];
        MaAtrRsiWP[i] = MaAtrRsi[i];
    }

    i = counted;
    tr = TrLevelSlow[i + 1];
    rsi1 = RsiMa[i + 1];

    while (i >= 0){
        rsi0 = RsiMa[i];
        MaAtrRsiWP[i] = MaAtrRsiWP[i] * (2.0 / (1 + Wilders_Period)) + (1 - (2.0 / (1 + Wilders_Period))) * MaAtrRsiWP[i + 1];
        dar = MaAtrRsiWP[i] * WF;

        dv = tr;
        if (rsi0 < tr){
            tr = rsi0 + dar;
            if (rsi1 < dv)
                if (tr > dv)
                    tr = dv;
        }
        else if (rsi0 > tr){
            tr = rsi0 - dar;
            if (rsi1 > dv)
                if (tr < dv)
                    tr = dv;
        }
        TrLevelSlow[i] = tr;
        rsi1 = rsi0;

        i--;
    }

    return(rates_total);
}

double getIndicatorValue(int indyHandle, int bufferIndex, int shift){
   double buffer[];

   if(CopyBuffer(indyHandle, bufferIndex, shift, 1, buffer) < 0) {
      PrintFormat("Failed to copy data from the indicator, error code %d", GetLastError());
      return(0);
   }

   double val = buffer[0];
   return (val >= 0 && val <= 100) ? val : 0;
}
"#.to_string()
}

// ── BT_SchaffTrendCycle ──

fn gen_mql5_schaff_trend_cycle() -> String {
r#"//+------------------------------------------------------------------+
//|                                          BT_SchaffTrendCycle.mq5 |
//|                           copyright "Copyright 2017, mladen"     |
//|                           mladenfx@gmail.com                     |
//+------------------------------------------------------------------+
#property copyright   "Copyright 2017, mladen"
#property link        "mladenfx@gmail.com"
#property description "Schaff Trend Cycle"
#property version     "1.00"
//------------------------------------------------------------------
#property indicator_separate_window
#property indicator_buffers 6
#property indicator_plots   1
#property indicator_label1  "Schaff Trend Cycle value"
#property indicator_type1   DRAW_COLOR_LINE
#property indicator_color1  clrSilver,clrLimeGreen,clrOrange
#property indicator_width1  2
//
//-----------------
//
enum enPrices
  {
   pr_close,      // Close
   pr_open,       // Open
   pr_high,       // High
   pr_low,        // Low
   pr_median,     // Median
   pr_typical,    // Typical
   pr_weighted,   // Weighted
   pr_average,    // Average (high+low+open+close)/4
   pr_medianb,    // Average median body (open+close)/2
   pr_tbiased,    // Trend biased price
   pr_tbiased2,   // Trend biased (extreme) price
   pr_haclose,    // Heiken Ashi close
   pr_haopen ,    // Heiken Ashi open
   pr_hahigh,     // Heiken Ashi high
   pr_halow,      // Heiken Ashi low
   pr_hamedian,   // Heiken Ashi median
   pr_hatypical,  // Heiken Ashi typical
   pr_haweighted, // Heiken Ashi weighted
   pr_haaverage,  // Heiken Ashi average
   pr_hamedianb,  // Heiken Ashi median body
   pr_hatbiased,  // Heiken Ashi trend biased price
   pr_hatbiased2  // Heiken Ashi trend biased (extreme) price
  };
// input parameters
input int       SchaffPeriod = 10;       // Schaff period
input int       FastEma      = 20;       // Fast EMA period
input int       SlowEma      = 50;       // Slow EMA period
double    SmoothPeriod = 3;        // Smoothing period
enPrices  Price        = pr_close; // Price

double  val[],valc[],macd[],fastk1[],fastd1[],fastk2[];
//+------------------------------------------------------------------+
//|                                                                  |
//+------------------------------------------------------------------+
void OnInit()
  {
   SetIndexBuffer(0,val,INDICATOR_DATA);
   SetIndexBuffer(1,valc,INDICATOR_COLOR_INDEX);
   SetIndexBuffer(2,macd,INDICATOR_CALCULATIONS);
   SetIndexBuffer(3,fastk1,INDICATOR_CALCULATIONS);
   SetIndexBuffer(4,fastk2,INDICATOR_CALCULATIONS);
   SetIndexBuffer(5,fastd1,INDICATOR_CALCULATIONS);
   IndicatorSetString(INDICATOR_SHORTNAME,"SqSchaffTrendCycle ("+(string)SchaffPeriod+","+(string)FastEma+","+(string)SlowEma+")");
  }
//+------------------------------------------------------------------+
//|                                                                  |
//+------------------------------------------------------------------+
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
   if(Bars(_Symbol,_Period)<rates_total) return(-1);
//
//
//
   //double alpha=2.0/(1.0+SmoothPeriod);
   double alpha=0.5;
   int i=(int)MathMax(prev_calculated-1,0); for(; i<rates_total && !_StopFlag; i++)
     {
      double price=getPrice(Price,open,close,high,low,i,rates_total);
      macd[i]=iEma(price,FastEma,i,rates_total,0)-iEma(price,SlowEma,i,rates_total,1);
      int    start    = MathMax(i-SchaffPeriod+1,0);
      double lowMacd  = macd[ArrayMinimum(macd,start,SchaffPeriod)];
      double highMacd = macd[ArrayMaximum(macd,start,SchaffPeriod)]-lowMacd;
      fastk1[i] = (highMacd > 0) ? 100*((macd[i]-lowMacd)/highMacd) : (i>0) ? fastk1[i-1] : 0;
      fastd1[i] = (i>0) ? fastd1[i-1]+alpha*(fastk1[i]-fastd1[i-1]) : fastk1[i];
      double lowStoch  = fastd1[ArrayMinimum(fastd1,start,SchaffPeriod)];
      double highStoch = fastd1[ArrayMaximum(fastd1,start,SchaffPeriod)]-lowStoch;
      fastk2[i] = (highStoch > 0) ? 100*((fastd1[i]-lowStoch)/highStoch) : (i>0) ? fastk2[i-1] : 0;
      val[i]    = (i>0) ?  val[i-1]+alpha*(fastk2[i]-val[i-1]) : fastk2[i];
      valc[i]   = (i>0) ? (val[i]>val[i-1]) ? 1 : (val[i]<val[i-1]) ? 2 : 0 : 0;
     }
   return(i);
  }
//------------------------------------------------------------------
// custom functions
//------------------------------------------------------------------
double workEma[][2];
//+------------------------------------------------------------------+
//|                                                                  |
//+------------------------------------------------------------------+
double iEma(double price,double period,int r,int _bars,int instanceNo=0)
  {
   if(ArrayRange(workEma,0)!=_bars) ArrayResize(workEma,_bars);

   workEma[r][instanceNo]=price;
   if(r>0 && period>1)
      workEma[r][instanceNo]=workEma[r-1][instanceNo]+(2.0/(1.0+period))*(price-workEma[r-1][instanceNo]);
   return(workEma[r][instanceNo]);
  }
//
//----------------------
//
#define _pricesInstances 1
#define _pricesSize      4
double workHa[][_pricesInstances*_pricesSize];
//+------------------------------------------------------------------+
//|                                                                  |
//+------------------------------------------------------------------+
double getPrice(int tprice,const double &open[],const double &close[],const double &high[],const double &low[],int i,int _bars,int instanceNo=0)
  {
   if(tprice>=pr_haclose)
     {
      if(ArrayRange(workHa,0)!=_bars) ArrayResize(workHa,_bars); instanceNo*=_pricesSize;
      double haOpen;
      if(i>0)
         haOpen  = (workHa[i-1][instanceNo+2] + workHa[i-1][instanceNo+3])/2.0;
      else   haOpen  = (open[i]+close[i])/2;
      double haClose = (open[i] + high[i] + low[i] + close[i]) / 4.0;
      double haHigh  = MathMax(high[i], MathMax(haOpen,haClose));
      double haLow   = MathMin(low[i] , MathMin(haOpen,haClose));

      if(haOpen  <haClose) { workHa[i][instanceNo+0] = haLow;  workHa[i][instanceNo+1] = haHigh; }
      else                 { workHa[i][instanceNo+0] = haHigh; workHa[i][instanceNo+1] = haLow;  }
      workHa[i][instanceNo+2] = haOpen;
      workHa[i][instanceNo+3] = haClose;
      //
      //--------------------
      //
      switch(tprice)
        {
         case pr_haclose:     return(haClose);
         case pr_haopen:      return(haOpen);
         case pr_hahigh:      return(haHigh);
         case pr_halow:       return(haLow);
         case pr_hamedian:    return((haHigh+haLow)/2.0);
         case pr_hamedianb:   return((haOpen+haClose)/2.0);
         case pr_hatypical:   return((haHigh+haLow+haClose)/3.0);
         case pr_haweighted:  return((haHigh+haLow+haClose+haClose)/4.0);
         case pr_haaverage:   return((haHigh+haLow+haClose+haOpen)/4.0);
         case pr_hatbiased:
            if(haClose>haOpen)
            return((haHigh+haClose)/2.0);
            else  return((haLow+haClose)/2.0);
         case pr_hatbiased2:
            if(haClose>haOpen)  return(haHigh);
            if(haClose<haOpen)  return(haLow);
            return(haClose);
        }
     }
//
//--------------------------
//
   switch(tprice)
     {
      case pr_close:     return(close[i]);
      case pr_open:      return(open[i]);
      case pr_high:      return(high[i]);
      case pr_low:       return(low[i]);
      case pr_median:    return((high[i]+low[i])/2.0);
      case pr_medianb:   return((open[i]+close[i])/2.0);
      case pr_typical:   return((high[i]+low[i]+close[i])/3.0);
      case pr_weighted:  return((high[i]+low[i]+close[i]+close[i])/4.0);
      case pr_average:   return((high[i]+low[i]+close[i]+open[i])/4.0);
      case pr_tbiased:
         if(close[i]>open[i])
         return((high[i]+close[i])/2.0);
         else  return((low[i]+close[i])/2.0);
      case pr_tbiased2:
         if(close[i]>open[i]) return(high[i]);
         if(close[i]<open[i]) return(low[i]);
         return(close[i]);
     }
   return(0);
  }
//+------------------------------------------------------------------+
"#.to_string()
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

/// Emit MQL5 code to initialize `g_trailing_dist` at entry time.
/// Called inside the `if(g_trade.Buy/Sell(...))` block where `entry` and `sl` are in scope.
fn emit_trailing_dist_init(
    strategy: &crate::models::sr_result::SrStrategy,
    out: &mut String,
) {
    use crate::models::strategy::TrailingStopType;
    use std::fmt::Write;
    if let Some(ts) = &strategy.trailing_stop {
        match ts.ts_type {
            TrailingStopType::ATR => {
                let period = ts.atr_period.unwrap_or(14);
                writeln!(out, "            g_trailing_dist = g_atr{period} * InpTSAtrMult;").ok();
            }
            TrailingStopType::RiskReward => {
                writeln!(out, "            g_trailing_dist = (sl != 0.0) ? MathAbs(entry - sl) * InpTSRR : 0.0;").ok();
            }
        }
    }
}

/// Generate an MQL5 Expert Advisor from a Symbolic Regression strategy.
/// The EA evaluates the three formula trees at each new bar using BT_* custom
/// indicators and applies the configured position sizing, SL, TP, and costs.
pub fn generate_sr_mql5(
    strategy: &crate::models::sr_result::SrStrategy,
    name: &str,
) -> Result<CodeGenerationResult, AppError> {
    use crate::engine::sr::tree::format_tree;
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
            SrNode::Constant(v) => {
                if v.abs() < 1e-4 && *v != 0.0 {
                    format!("{:e}", v)
                } else {
                    format!("{:.6}", v)
                }
            }
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
    writeln!(out, "// ═══════════════ BACKTESTER MATCHING GUIDE ═══════════════════════").ok();
    writeln!(out, "// Strategy Tester Mode : \"Open prices only\"").ok();
    writeln!(out, "//   (matches Backtester bar-close decision model)").ok();
    writeln!(out, "//   \"Every tick\"  → more SL/TP triggers → lower profit than Backtester").ok();
    writeln!(out, "//   \"OHLC on M1\" → intermediate; use if Backtester uses M1 simulation").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Commission  : InpCommission reflects the value configured in Backtester.").ok();
    writeln!(out, "//               Set InpCommission=0 if your broker already charges commission").ok();
    writeln!(out, "//               in Tester settings — otherwise it will be double-counted.").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Slippage    : InpSlippagePips adjusts entry price to simulate slippage.").ok();
    writeln!(out, "//               Set to 0 if you prefer natural market execution.").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// Data        : Use same symbol / timeframe / date range as Backtester.").ok();
    writeln!(out, "//               MT5 history server downloads bid prices ✓").ok();
    writeln!(out, "//               CSV from Yahoo/mid-price sources → spreads will differ.").ok();
    writeln!(out, "// ═══════════════════════════════════════════════════════════════════").ok();
    writeln!(out, "//").ok();
    writeln!(out, "// ═══════════════ SR FORMULAS (human-readable) ════════════════════").ok();
    writeln!(out, "// Entry Long  : {}", format_tree(&strategy.entry_long)).ok();
    writeln!(out, "// Entry Short : {}", format_tree(&strategy.entry_short)).ok();
    writeln!(out, "// Exit        : {}", format_tree(&strategy.exit)).ok();
    writeln!(out, "// Long  threshold : {:.6}", strategy.long_threshold).ok();
    writeln!(out, "// Short threshold : {:.6}", strategy.short_threshold).ok();
    writeln!(out, "// ═══════════════════════════════════════════════════════════════════").ok();
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
    writeln!(out, "input int    InpMagicNumber  = 88888;  // Magic number").ok();
    writeln!(out, "input int    InpSlippage      = 30;     // Max slippage in points (prevents requotes)").ok();

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
    // Time filter inputs
    if let Some(th) = &strategy.trading_hours {
        writeln!(out, "// ── Trading hours ─────────────────────────────────────────────────").ok();
        writeln!(out, "input int    InpStartHour   = {};  // Trading start hour", th.start_hour).ok();
        writeln!(out, "input int    InpStartMinute = {};  // Trading start minute", th.start_minute).ok();
        writeln!(out, "input int    InpEndHour     = {};  // Trading end hour", th.end_hour).ok();
        writeln!(out, "input int    InpEndMinute   = {};  // Trading end minute", th.end_minute).ok();
    }
    if let Some(ct) = &strategy.close_trades_at {
        writeln!(out, "// ── Force-close time ──────────────────────────────────────────────").ok();
        writeln!(out, "input int    InpCloseHour   = {};  // Force-close hour (0-23)", ct.hour).ok();
        writeln!(out, "input int    InpCloseMinute = {};  // Force-close minute (0-59)", ct.minute).ok();
    }
    if let Some(n) = strategy.max_trades_per_day {
        writeln!(out, "input int    InpMaxDailyTrades = {};  // Max new trades per day", n).ok();
    }
    if strategy.use_exit_formula && strategy.exit_dead_zone != 0.0 {
        writeln!(out, "input double InpExitDeadZone  = {:.10};  // Exit dead zone (signal must cross ± this)", strategy.exit_dead_zone).ok();
    }
    if let Some(n) = strategy.max_bars_open {
        writeln!(out, "input int    InpMaxBarsOpen    = {};  // Close position after N bars", n).ok();
    }
    if let Some(n) = strategy.min_bars_between_trades {
        writeln!(out, "input int    InpCooldownBars   = {};  // Min bars between trades", n).ok();
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
    if strategy.use_exit_formula {
        writeln!(out, "double   g_exit_prev          = 0.0;    // exit formula value on previous bar (matches Rust init)").ok();
    }
    // Entry bar time (for exit guard: no exit within 1 bar of entry)
    writeln!(out, "datetime g_entry_bar_time = 0;      // bar open time when position was opened").ok();
    if need_trailing {
        writeln!(out, "double   g_trailing_dist   = 0.0;    // trailing stop distance (fixed at entry, matches Rust)").ok();
    }
    if strategy.min_bars_between_trades.is_some() {
        writeln!(out, "datetime g_last_exit_bar_time = 0;  // bar open time when last position was closed").ok();
    }
    // Daily trade counter for max_trades_per_day
    if strategy.max_trades_per_day.is_some() {
        writeln!(out, "int      g_daily_count     = 0;      // trades opened today").ok();
        writeln!(out, "int      g_last_day        = 0;      // last date as YYYYMMDD").ok();
    }
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
    writeln!(out, "   g_trade.SetDeviationInPoints(InpSlippage);").ok();
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
    if strategy.use_exit_formula {
        writeln!(out, "   g_exit_prev         = 0.0;").ok();
    }
    writeln!(out, "   g_entry_bar_time    = 0;").ok();
    if strategy.max_trades_per_day.is_some() {
        writeln!(out, "   g_daily_count    = 0;").ok();
        writeln!(out, "   g_last_day       = 0;").ok();
    }
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

    let needs_dt = strategy.trading_hours.is_some()
        || strategy.close_trades_at.is_some()
        || strategy.max_trades_per_day.is_some();
    if needs_dt {
        writeln!(out, "   MqlDateTime dt;").ok();
        writeln!(out, "   TimeToStruct(cur_bar, dt);").ok();
        writeln!(out).ok();
    }
    if strategy.max_trades_per_day.is_some() {
        writeln!(out, "   // Daily trade count reset").ok();
        writeln!(out, "   int _today = dt.year * 10000 + dt.mon * 100 + dt.day;").ok();
        writeln!(out, "   if(_today != g_last_day) {{ g_daily_count = 0; g_last_day = _today; }}").ok();
        writeln!(out).ok();
    }

    // Read indicator buffers at shift=1 (previous completed bar)
    writeln!(out, "   // ── Read indicator values at shift=1 (previous bar, matching backtester) ──").ok();
    writeln!(out, "   double _b[];").ok();
    writeln!(out, "   bool _all_ready = true;  // set false if any CopyBuffer fails (warmup guard)").ok();
    for (key, buf) in &used_buffers {
        if let Some(v) = var_map.get(key) {
            writeln!(out, "   if(CopyBuffer(g_{v}_handle, {buf}, 1, 1, _b) == 1) g_{v}_{buf} = _b[0]; else {{ Print(\"WARN: CopyBuffer failed for {v} buf={buf}\"); _all_ready = false; }}").ok();
        }
    }
    // Read ATR values at shift=1
    if !atr_periods.is_empty() {
        writeln!(out).ok();
        writeln!(out, "   // ── Read ATR values at shift=1 ───────────────────────────────────").ok();
        for &period in &atr_periods {
            writeln!(out, "   if(CopyBuffer(g_atr{period}_handle, 0, 1, 1, _b) == 1) g_atr{period} = _b[0]; else _all_ready = false;").ok();
        }
    }
    writeln!(out, "   if(!_all_ready) return;  // indicators warming up — skip until all data is ready").ok();
    writeln!(out).ok();

    // Evaluate SR formulas
    writeln!(out, "   // ── Evaluate SR formulas ──────────────────────────────────────────").ok();
    writeln!(out, "   double signal_long  = {expr_long};").ok();
    writeln!(out, "   double signal_short = {expr_short};").ok();
    if strategy.use_exit_formula {
        writeln!(out, "   double signal_exit  = {expr_exit};").ok();
        writeln!(out, "   if(!MathIsValidNumber(signal_exit)) signal_exit = g_exit_prev;  // NaN/Inf guard — keep previous value").ok();
    }
    writeln!(out).ok();

    if strategy.use_exit_formula {
        // Exit sign-change detection with dead zone (matches runner.rs exactly).
        // runner.rs: (prev > dz && cur < -dz) || (prev < -dz && cur > dz)
        let dz = strategy.exit_dead_zone;
        writeln!(out, "   // ── Exit logic ────────────────────────────────────────────────────").ok();
        writeln!(out, "   // Sign change in exit formula triggers close (matches backtester runner.rs).").ok();
        if dz != 0.0 {
            writeln!(out, "   bool sign_changed = (g_exit_prev > InpExitDeadZone && signal_exit < -InpExitDeadZone)").ok();
            writeln!(out, "                    || (g_exit_prev < -InpExitDeadZone && signal_exit > InpExitDeadZone);").ok();
        } else {
            writeln!(out, "   bool sign_changed = (g_exit_prev > 0.0 && signal_exit < 0.0)").ok();
            writeln!(out, "                    || (g_exit_prev < 0.0 && signal_exit > 0.0);").ok();
        }
        writeln!(out, "   // Exit guard: do NOT exit on the bar immediately after entry").ok();
        writeln!(out, "   // (matches condition: i > pos.entry_bar + 1 in the backtester).").ok();
        writeln!(out, "   datetime prev_bar = iTime(_Symbol, PERIOD_CURRENT, 1);").ok();
        writeln!(out, "   bool exit_guard_ok = (g_entry_bar_time == 0 || prev_bar != g_entry_bar_time);").ok();
        // Initialize prev to 0.0 on first bar (matches Rust: prev_exit_signal = 0.0)
        writeln!(out, "   bool do_exit = sign_changed && exit_guard_ok;").ok();
        writeln!(out, "   g_exit_prev = signal_exit;").ok();
    } else {
        writeln!(out, "   // ── Exit logic (formula disabled — close via SL/TP/time only) ───────").ok();
        writeln!(out, "   bool do_exit = false;").ok();
    }
    writeln!(out).ok();

    // Force-close at specified time (after state update so sign-change machine stays accurate)
    if strategy.close_trades_at.is_some() {
        writeln!(out, "   // Force-close all positions at or after InpCloseHour:InpCloseMinute").ok();
        writeln!(out, "   if(dt.hour * 60 + dt.min >= InpCloseHour * 60 + InpCloseMinute)").ok();
        writeln!(out, "   {{").ok();
        writeln!(out, "      for(int _ci = PositionsTotal() - 1; _ci >= 0; _ci--)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         ulong _ct = PositionGetTicket(_ci);").ok();
        writeln!(out, "         if(_ct == 0) continue;").ok();
        writeln!(out, "         if((long)PositionGetInteger(POSITION_MAGIC) != InpMagicNumber) continue;").ok();
        writeln!(out, "         if(PositionGetString(POSITION_SYMBOL) != _Symbol) continue;").ok();
        writeln!(out, "         g_trade.PositionClose(_ct);").ok();
        writeln!(out, "      }}").ok();
        if strategy.min_bars_between_trades.is_some() {
            writeln!(out, "      g_last_exit_bar_time = cur_bar;").ok();
        }
        writeln!(out, "      return;").ok();
        writeln!(out, "   }}").ok();
        writeln!(out).ok();
    }

    // Manage open positions: handle exit signal + trailing stop
    writeln!(out, "   bool _had_pos  = (g_entry_bar_time != 0);  // track if we had a position at start of bar").ok();
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
    // max_bars_open: force-close positions held longer than N bars
    if strategy.max_bars_open.is_some() {
        writeln!(out, "      if(g_entry_bar_time > 0 && InpMaxBarsOpen > 0)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         int bars_held = Bars(_Symbol, PERIOD_CURRENT, g_entry_bar_time, TimeCurrent()) - 1;").ok();
        writeln!(out, "         if(bars_held >= InpMaxBarsOpen) do_exit = true;").ok();
        writeln!(out, "      }}").ok();
    }
    writeln!(out, "      if(do_exit)").ok();
    writeln!(out, "      {{").ok();
    writeln!(out, "         g_trade.PositionClose(ticket);").ok();
    if strategy.min_bars_between_trades.is_some() {
        writeln!(out, "         g_last_exit_bar_time = cur_bar;").ok();
    }
    writeln!(out, "      }}").ok();
    writeln!(out, "   }}").ok();
    writeln!(out).ok();

    // Trailing stop update (after exit check, before entry)
    if need_trailing {
        writeln!(out, "   // ── Trailing stop ─────────────────────────────────────────────────").ok();
        writeln!(out, "   if(!do_exit && (has_long || has_short)) SR_ManageTrailingStop();").ok();
        writeln!(out).ok();
    }

    // Detect broker-side closes (SL/TP hit between bars) and update cooldown
    if strategy.min_bars_between_trades.is_some() {
        writeln!(out, "   // ── Detect SL/TP/TS close that happened between bars ──────────────").ok();
        writeln!(out, "   if(_had_pos && !has_long && !has_short && !do_exit)").ok();
        writeln!(out, "      g_last_exit_bar_time = cur_bar;  // position was closed by SL/TP/TS").ok();
        writeln!(out).ok();
    }

    // Entry logic
    writeln!(out, "   // ── Entry logic ───────────────────────────────────────────────────").ok();
    writeln!(out, "   // If exit just fired, skip entry this bar (same as backtester).").ok();
    writeln!(out, "   if(!do_exit && !has_long && !has_short)").ok();
    writeln!(out, "   {{").ok();

    // Cooldown guard — min bars between trades
    if strategy.min_bars_between_trades.is_some() {
        writeln!(out, "      bool _cooldown_ok = true;").ok();
        writeln!(out, "      if(g_last_exit_bar_time > 0 && InpCooldownBars > 0)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         int _bars_since_exit = Bars(_Symbol, PERIOD_CURRENT, g_last_exit_bar_time, TimeCurrent()) - 1;").ok();
        writeln!(out, "         if(_bars_since_exit < InpCooldownBars) _cooldown_ok = false;").ok();
        writeln!(out, "      }}").ok();
        writeln!(out, "      if(_cooldown_ok)").ok();
        writeln!(out, "      {{").ok();
    }

    // Trading hours guard — wrap entry block if configured
    if strategy.trading_hours.is_some() {
        writeln!(out, "      int _cur_min   = dt.hour * 60 + dt.min;").ok();
        writeln!(out, "      int _start_min = InpStartHour * 60 + InpStartMinute;").ok();
        writeln!(out, "      int _end_min   = InpEndHour   * 60 + InpEndMinute;").ok();
        writeln!(out, "      bool _inHours = (_start_min <= _end_min)").ok();
        writeln!(out, "                    ? (_cur_min >= _start_min && _cur_min < _end_min)").ok();
        writeln!(out, "                    : (_cur_min >= _start_min || _cur_min < _end_min);").ok();
        writeln!(out, "      if(_inHours)").ok();
        writeln!(out, "      {{").ok();
    }
    if strategy.max_trades_per_day.is_some() {
        writeln!(out, "      if(g_daily_count < InpMaxDailyTrades)").ok();
        writeln!(out, "      {{").ok();
    }

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
        writeln!(out, "         {{").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        if need_trailing {
            emit_trailing_dist_init(strategy, &mut out);
        }
        if strategy.max_trades_per_day.is_some() {
            writeln!(out, "            g_daily_count++;").ok();
        }
        writeln!(out, "         }}").ok();
        writeln!(out, "         else Print(\"BUY failed: \", GetLastError());").ok();
        writeln!(out, "      }}").ok();
        writeln!(out, "      else if(go_short)").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_BID);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_SELL, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_SELL, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Sell(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "         {{").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        if need_trailing {
            emit_trailing_dist_init(strategy, &mut out);
        }
        if strategy.max_trades_per_day.is_some() {
            writeln!(out, "            g_daily_count++;").ok();
        }
        writeln!(out, "         }}").ok();
        writeln!(out, "         else Print(\"SELL failed: \", GetLastError());").ok();
        writeln!(out, "      }}").ok();
    } else if allow_long {
        writeln!(out, "      if((signal_long > InpLongThreshold) && MathIsValidNumber(signal_long))").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_ASK);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_BUY, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_BUY, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Buy(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "         {{").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        if need_trailing {
            emit_trailing_dist_init(strategy, &mut out);
        }
        if strategy.max_trades_per_day.is_some() {
            writeln!(out, "            g_daily_count++;").ok();
        }
        writeln!(out, "         }}").ok();
        writeln!(out, "         else Print(\"BUY failed: \", GetLastError());").ok();
        writeln!(out, "      }}").ok();
    } else if allow_short {
        writeln!(out, "      if((signal_short < InpShortThreshold) && MathIsValidNumber(signal_short))").ok();
        writeln!(out, "      {{").ok();
        writeln!(out, "         double entry = SymbolInfoDouble(_Symbol, SYMBOL_BID);").ok();
        writeln!(out, "         double sl    = SR_CalcSL(ORDER_TYPE_SELL, entry);").ok();
        writeln!(out, "         double tp    = SR_CalcTP(ORDER_TYPE_SELL, entry, sl);").ok();
        writeln!(out, "         double lots  = SR_CalcLots(entry, sl);").ok();
        writeln!(out, "         if(g_trade.Sell(lots, _Symbol, entry, sl, tp, \"{ea_name}\"))").ok();
        writeln!(out, "         {{").ok();
        writeln!(out, "            g_entry_bar_time = cur_bar;").ok();
        if need_trailing {
            emit_trailing_dist_init(strategy, &mut out);
        }
        if strategy.max_trades_per_day.is_some() {
            writeln!(out, "            g_daily_count++;").ok();
        }
        writeln!(out, "         }}").ok();
        writeln!(out, "         else Print(\"SELL failed: \", GetLastError());").ok();
        writeln!(out, "      }}").ok();
    }

    if strategy.max_trades_per_day.is_some() {
        writeln!(out, "      }} // end daily limit guard").ok();
    }
    if strategy.trading_hours.is_some() {
        writeln!(out, "      }} // end inHours guard").ok();
    }
    if strategy.min_bars_between_trades.is_some() {
        writeln!(out, "      }} // end cooldown guard").ok();
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
    if let Some(_ts) = &strategy.trailing_stop {
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
        writeln!(out, "      ENUM_POSITION_TYPE ptype = (ENUM_POSITION_TYPE)PositionGetInteger(POSITION_TYPE);").ok();
        writeln!(out).ok();

        // Use fixed trailing distance computed at entry (matches Rust: trailing_stop_distance is set once)
        writeln!(out, "      // Trailing distance fixed at entry (matches backtester)").ok();
        writeln!(out, "      double trailDist = g_trailing_dist;").ok();
        writeln!(out, "      if(trailDist <= 0.0) continue;").ok();

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
        // ── Non-SQX indicators (BT_* custom files) ──
        IndicatorType::SMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_SMA\", Inp_{}_period)", var),
        IndicatorType::EMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_EMA\", Inp_{}_period)", var),
        IndicatorType::RSI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_RSI\", Inp_{}_period)", var),
        IndicatorType::MACD => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_MACD\", Inp_{0}_fast, Inp_{0}_slow, Inp_{0}_signal)", var),
        IndicatorType::BollingerBands => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_BollingerBands\", Inp_{0}_period, Inp_{0}_stddev)", var),
        IndicatorType::DeMarker => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_DeMarker\", Inp_{}_period)", var),
        IndicatorType::AwesomeOscillator => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_AwesomeOscillator\")".to_string(),
        IndicatorType::BarRange => "iCustom(_Symbol, PERIOD_CURRENT, \"BT_BarRange\")".to_string(),
        IndicatorType::Momentum => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_Momentum\", Inp_{}_period)", var),
        IndicatorType::StdDev => format!("iCustom(_Symbol, PERIOD_CURRENT, \"BT_StdDev\", Inp_{}_period)", var),
        // ── SQX indicators ──
        IndicatorType::ATR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqATR\", Inp_{}_period)", var),
        IndicatorType::Stochastic => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqStochastic\", Inp_{0}_k, Inp_{0}_d, 3, MODE_SMA, STO_LOWHIGH)", var),
        IndicatorType::ADX => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqADX\", Inp_{}_period)", var),
        IndicatorType::CCI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqCCI\", Inp_{}_period, PRICE_TYPICAL)", var),
        IndicatorType::WilliamsR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqWPR\", Inp_{}_period)", var),
        IndicatorType::ParabolicSAR => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqParabolicSAR\", Inp_{0}_af, Inp_{0}_max)", var),
        IndicatorType::ROC => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqROC\", Inp_{}_period)", var),
        IndicatorType::Ichimoku => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqIchimoku\", Inp_{0}_tenkan, Inp_{0}_kijun, Inp_{0}_senkou)", var),
        IndicatorType::KeltnerChannel => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqKeltnerChannel\", Inp_{0}_period, Inp_{0}_mult)", var),
        IndicatorType::SuperTrend => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqSuperTrend\", 1, Inp_{0}_period, Inp_{0}_mult)", var),
        IndicatorType::LaguerreRSI => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqLaguerreRSI\", Inp_{}_gamma)", var),
        IndicatorType::BearsPower => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqBearsPower\", Inp_{}_period)", var),
        IndicatorType::BullsPower => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqBullsPower\", Inp_{}_period)", var),
        IndicatorType::TrueRange => "iCustom(_Symbol, PERIOD_CURRENT, \"SqTrueRange\")".to_string(),
        IndicatorType::LinearRegression => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqLinReg\", Inp_{}_period, PRICE_CLOSE)", var),
        IndicatorType::Fractal => "iCustom(_Symbol, PERIOD_CURRENT, \"SqFractal\", 3)".to_string(),
        IndicatorType::HeikenAshi => "iCustom(_Symbol, PERIOD_CURRENT, \"SqHeikenAshi\")".to_string(),
        IndicatorType::GannHiLo => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqGannHiLo\", Inp_{}_period)", var),
        IndicatorType::HullMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqHullMovingAverage\", Inp_{}_period, 2.0, PRICE_CLOSE)", var),
        IndicatorType::UlcerIndex => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqUlcerIndex\", Inp_{}_period, 1)", var),
        IndicatorType::Vortex => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqVortex\", Inp_{}_period)", var),
        IndicatorType::Aroon => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqAroon\", Inp_{}_period, 0)", var),
        IndicatorType::HighestInRange => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqHighest\", Inp_{}_period, PRICE_HIGH)", var),
        IndicatorType::LowestInRange => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqLowest\", Inp_{}_period, PRICE_LOW)", var),
        IndicatorType::Reflex => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqReflex\", Inp_{}_period)", var),
        IndicatorType::AvgVolume => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqAvgVolume\", Inp_{}_period)", var),
        IndicatorType::BBWidthRatio => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqBBWidthRatio\", Inp_{0}_period, Inp_{0}_stddev, PRICE_CLOSE)", var),
        IndicatorType::EfficiencyRatio => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqEfficiencyRatio\", Inp_{}_period)", var),
        IndicatorType::HighestIndex => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqHighestIndex\", Inp_{}_period, PRICE_HIGH)", var),
        IndicatorType::KAMA => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqKAMA\", Inp_{0}_period, Inp_{0}_fast, Inp_{0}_slow, 0)", var),
        IndicatorType::LowestIndex => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqLowestIndex\", Inp_{}_period, PRICE_LOW)", var),
        IndicatorType::QQE => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqQQE\", Inp_{0}_period, Inp_{0}_sf, Inp_{0}_wf)", var),
        IndicatorType::SchaffTrendCycle => format!("iCustom(_Symbol, PERIOD_CURRENT, \"SqSchaffTrendCycle\", Inp_{0}_period, Inp_{0}_fast, Inp_{0}_slow, 3.0)", var),
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
