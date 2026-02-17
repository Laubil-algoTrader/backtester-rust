use std::collections::HashMap;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::strategy::{
    Comparator, IndicatorConfig, LogicalOperator, Operand, OperandType, PriceField, Rule, Strategy,
};

use super::indicators::{compute_indicator, IndicatorOutput};

/// Cache of pre-computed indicator values, keyed by `IndicatorConfig::cache_key()`.
pub type IndicatorCache = HashMap<String, IndicatorOutput>;

/// Pre-compute all indicators referenced in a strategy's rules.
/// Returns a cache that can be queried during rule evaluation.
pub fn pre_compute_indicators(
    strategy: &Strategy,
    candles: &[Candle],
) -> Result<IndicatorCache, AppError> {
    let mut cache = IndicatorCache::new();
    let mut seen = std::collections::HashSet::new();

    // Collect all indicator configs from all 4 rule lists
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter());
    for rule in all_rules {
        collect_indicator_from_operand(&rule.left_operand, &mut seen, &mut cache, candles)?;
        collect_indicator_from_operand(&rule.right_operand, &mut seen, &mut cache, candles)?;
    }

    Ok(cache)
}

fn collect_indicator_from_operand(
    operand: &Operand,
    seen: &mut std::collections::HashSet<String>,
    cache: &mut IndicatorCache,
    candles: &[Candle],
) -> Result<(), AppError> {
    if operand.operand_type == OperandType::Indicator {
        if let Some(ref config) = operand.indicator {
            let key = config.cache_key();
            if seen.insert(key.clone()) {
                let output = compute_indicator(config, candles)?;
                cache.insert(key, output);
            }
        }
    }
    Ok(())
}

/// Evaluate a list of rules at a given bar index.
/// Rules are connected by AND/OR logic. Returns true if all conditions are met.
pub fn evaluate_rules(
    rules: &[Rule],
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
) -> bool {
    if rules.is_empty() {
        return false;
    }

    let mut result = evaluate_single_rule(&rules[0], bar_index, cache, candles);

    for i in 1..rules.len() {
        let prev_operator = rules[i - 1]
            .logical_operator
            .unwrap_or(LogicalOperator::And);
        let current = evaluate_single_rule(&rules[i], bar_index, cache, candles);

        match prev_operator {
            LogicalOperator::And => result = result && current,
            LogicalOperator::Or => result = result || current,
        }
    }

    result
}

/// Evaluate a single rule at a bar index.
fn evaluate_single_rule(
    rule: &Rule,
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
) -> bool {
    let left = resolve_operand(&rule.left_operand, bar_index, cache, candles);
    let right = resolve_operand(&rule.right_operand, bar_index, cache, candles);

    // NaN values should not trigger any comparison
    if left.is_nan() || right.is_nan() {
        return false;
    }

    match rule.comparator {
        Comparator::GreaterThan => left > right,
        Comparator::LessThan => left < right,
        Comparator::GreaterOrEqual => left >= right,
        Comparator::LessOrEqual => left <= right,
        Comparator::Equal => (left - right).abs() < f64::EPSILON,
        Comparator::CrossAbove => {
            if bar_index == 0 {
                return false;
            }
            let prev_left = resolve_operand(&rule.left_operand, bar_index - 1, cache, candles);
            let prev_right = resolve_operand(&rule.right_operand, bar_index - 1, cache, candles);
            if prev_left.is_nan() || prev_right.is_nan() {
                return false;
            }
            prev_left <= prev_right && left > right
        }
        Comparator::CrossBelow => {
            if bar_index == 0 {
                return false;
            }
            let prev_left = resolve_operand(&rule.left_operand, bar_index - 1, cache, candles);
            let prev_right = resolve_operand(&rule.right_operand, bar_index - 1, cache, candles);
            if prev_left.is_nan() || prev_right.is_nan() {
                return false;
            }
            prev_left >= prev_right && left < right
        }
    }
}

/// Resolve an operand's value at a specific bar index.
fn resolve_operand(
    operand: &Operand,
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
) -> f64 {
    let effective_index = if let Some(offset) = operand.offset {
        if bar_index < offset {
            return f64::NAN;
        }
        bar_index - offset
    } else {
        bar_index
    };

    if effective_index >= candles.len() {
        return f64::NAN;
    }

    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                let key = config.cache_key();
                if let Some(output) = cache.get(&key) {
                    get_indicator_value(output, config, effective_index)
                } else {
                    f64::NAN
                }
            } else {
                f64::NAN
            }
        }
        OperandType::Price => {
            let candle = &candles[effective_index];
            match operand.price_field {
                Some(PriceField::Open) => candle.open,
                Some(PriceField::High) => candle.high,
                Some(PriceField::Low) => candle.low,
                Some(PriceField::Close) => candle.close,
                None => candle.close,
            }
        }
        OperandType::Constant => operand.constant_value.unwrap_or(0.0),
    }
}

/// Get the appropriate value from an indicator output based on the output_field.
fn get_indicator_value(
    output: &IndicatorOutput,
    config: &IndicatorConfig,
    index: usize,
) -> f64 {
    match config.output_field.as_deref() {
        Some("signal") | Some("d") => {
            // MACD signal line, Stochastic %D
            output
                .secondary
                .as_ref()
                .and_then(|s| s.get(index).copied())
                .unwrap_or(f64::NAN)
        }
        Some("histogram") => {
            // MACD histogram
            output
                .tertiary
                .as_ref()
                .and_then(|s| s.get(index).copied())
                .unwrap_or(f64::NAN)
        }
        Some("upper") => {
            // Bollinger upper band
            output
                .secondary
                .as_ref()
                .and_then(|s| s.get(index).copied())
                .unwrap_or(f64::NAN)
        }
        Some("lower") => {
            // Bollinger lower band
            output
                .tertiary
                .as_ref()
                .and_then(|s| s.get(index).copied())
                .unwrap_or(f64::NAN)
        }
        Some("middle") | None | Some(_) => {
            // Default: primary output
            output
                .primary
                .get(index)
                .copied()
                .unwrap_or(f64::NAN)
        }
    }
}

/// Calculate the maximum lookback period needed for a strategy's indicators.
pub fn max_lookback(strategy: &Strategy) -> usize {
    let mut max = 0usize;
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter());
    for rule in all_rules {
        max = max.max(operand_lookback(&rule.left_operand));
        max = max.max(operand_lookback(&rule.right_operand));
    }
    // Add 1 for CrossAbove/CrossBelow which needs previous bar
    max + 1
}

fn operand_lookback(operand: &Operand) -> usize {
    let base = if operand.operand_type == OperandType::Indicator {
        if let Some(ref config) = operand.indicator {
            indicator_lookback(config)
        } else {
            0
        }
    } else {
        0
    };
    base + operand.offset.unwrap_or(0)
}

fn indicator_lookback(config: &IndicatorConfig) -> usize {
    use crate::models::strategy::IndicatorType::*;
    match config.indicator_type {
        SMA | EMA | CCI | WilliamsR => config.params.period.unwrap_or(14),
        RSI | ATR => config.params.period.unwrap_or(14) + 1,
        ROC => config.params.period.unwrap_or(14) + 1,
        MACD => {
            let slow = config.params.slow_period.unwrap_or(26);
            let signal = config.params.signal_period.unwrap_or(9);
            slow + signal
        }
        BollingerBands => config.params.period.unwrap_or(20),
        Stochastic => {
            let k = config.params.k_period.unwrap_or(14);
            let d = config.params.d_period.unwrap_or(3);
            k + d
        }
        ADX => config.params.period.unwrap_or(14) * 2 + 1,
        ParabolicSAR => 2,
        VWAP => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::strategy::*;

    fn make_candles(prices: &[f64]) -> Vec<Candle> {
        prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle {
                datetime: format!("2024-01-{:02} 00:00", (i % 28) + 1),
                open: p - 0.5,
                high: p + 1.0,
                low: p - 1.0,
                close: p,
                volume: 1000.0,
                ..Default::default()
            })
            .collect()
    }

    fn indicator_operand(indicator_type: IndicatorType, period: usize) -> Operand {
        Operand {
            operand_type: OperandType::Indicator,
            indicator: Some(IndicatorConfig {
                indicator_type,
                params: IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                output_field: None,
            }),
            price_field: None,
            constant_value: None,
            offset: None,
        }
    }

    fn constant_operand(value: f64) -> Operand {
        Operand {
            operand_type: OperandType::Constant,
            indicator: None,
            price_field: None,
            constant_value: Some(value),
            offset: None,
        }
    }

    fn price_operand(field: PriceField) -> Operand {
        Operand {
            operand_type: OperandType::Price,
            indicator: None,
            price_field: Some(field),
            constant_value: None,
            offset: None,
        }
    }

    #[test]
    fn test_evaluate_greater_than() {
        let candles = make_candles(&[10.0, 20.0, 30.0]);
        let rules = vec![Rule {
            id: "r1".to_string(),
            left_operand: price_operand(PriceField::Close),
            comparator: Comparator::GreaterThan,
            right_operand: constant_operand(15.0),
            logical_operator: None,
        }];
        let cache = IndicatorCache::new();
        assert!(!evaluate_rules(&rules, 0, &cache, &candles)); // 10 > 15 = false
        assert!(evaluate_rules(&rules, 1, &cache, &candles)); // 20 > 15 = true
    }

    #[test]
    fn test_evaluate_cross_above() {
        // Create prices where SMA(3) crosses above a constant
        // Prices: 10, 12, 14, 16, 18 → SMA(3) at idx 2=12, 3=14, 4=16
        let candles = make_candles(&[10.0, 12.0, 14.0, 16.0, 18.0]);
        let strategy = Strategy {
            id: "s1".to_string(),
            name: "test".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            long_entry_rules: vec![Rule {
                id: "r1".to_string(),
                left_operand: indicator_operand(IndicatorType::SMA, 3),
                comparator: Comparator::CrossAbove,
                right_operand: constant_operand(13.0),
                logical_operator: None,
            }],
            short_entry_rules: vec![],
            long_exit_rules: vec![],
            short_exit_rules: vec![],
            position_sizing: PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: 1.0,
            },
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
            trading_costs: TradingCosts {
                spread_pips: 0.0,
                commission_type: CommissionType::FixedPerLot,
                commission_value: 0.0,
                slippage_pips: 0.0,
                slippage_random: false,
            },
            trade_direction: TradeDirection::Both,
            trading_hours: None,
            max_daily_trades: None,
            close_trades_at: None,
        };

        let cache = pre_compute_indicators(&strategy, &candles).unwrap();
        // SMA(3): NaN, NaN, 12.0, 14.0, 16.0
        // CrossAbove 13.0: at idx 3 → prev=12.0 <= 13.0 AND curr=14.0 > 13.0 → true
        assert!(evaluate_rules(&strategy.long_entry_rules, 3, &cache, &candles));
        // At idx 4 → prev=14.0 > 13.0, so no cross
        assert!(!evaluate_rules(&strategy.long_entry_rules, 4, &cache, &candles));
    }

    #[test]
    fn test_evaluate_and_logic() {
        let candles = make_candles(&[50.0]);
        let rules = vec![
            Rule {
                id: "r1".to_string(),
                left_operand: price_operand(PriceField::Close),
                comparator: Comparator::GreaterThan,
                right_operand: constant_operand(40.0),
                logical_operator: Some(LogicalOperator::And),
            },
            Rule {
                id: "r2".to_string(),
                left_operand: price_operand(PriceField::Close),
                comparator: Comparator::LessThan,
                right_operand: constant_operand(60.0),
                logical_operator: None,
            },
        ];
        let cache = IndicatorCache::new();
        // 50 > 40 AND 50 < 60 → true
        assert!(evaluate_rules(&rules, 0, &cache, &candles));
    }

    #[test]
    fn test_evaluate_or_logic() {
        let candles = make_candles(&[50.0]);
        let rules = vec![
            Rule {
                id: "r1".to_string(),
                left_operand: price_operand(PriceField::Close),
                comparator: Comparator::GreaterThan,
                right_operand: constant_operand(100.0),
                logical_operator: Some(LogicalOperator::Or),
            },
            Rule {
                id: "r2".to_string(),
                left_operand: price_operand(PriceField::Close),
                comparator: Comparator::LessThan,
                right_operand: constant_operand(60.0),
                logical_operator: None,
            },
        ];
        let cache = IndicatorCache::new();
        // 50 > 100 = false OR 50 < 60 = true → true
        assert!(evaluate_rules(&rules, 0, &cache, &candles));
    }

    #[test]
    fn test_operand_with_offset() {
        let candles = make_candles(&[10.0, 20.0, 30.0]);
        let rules = vec![Rule {
            id: "r1".to_string(),
            left_operand: price_operand(PriceField::Close),
            comparator: Comparator::GreaterThan,
            right_operand: Operand {
                operand_type: OperandType::Price,
                indicator: None,
                price_field: Some(PriceField::Close),
                constant_value: None,
                offset: Some(1),
            },
            logical_operator: None,
        }];
        let cache = IndicatorCache::new();
        // Bar 2: close=30 > close[1]=20 → true
        assert!(evaluate_rules(&rules, 2, &cache, &candles));
        // Bar 0: offset=1 would be index -1 → NaN → false
        assert!(!evaluate_rules(&rules, 0, &cache, &candles));
    }
}
