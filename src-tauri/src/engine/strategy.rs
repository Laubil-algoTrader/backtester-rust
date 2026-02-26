use std::collections::HashMap;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::strategy::{
    CandlePatternType, Comparator, IndicatorConfig, LogicalOperator, Operand, OperandType,
    PriceField, Rule, Strategy, TimeField,
};

use super::indicators::{compute_indicator, IndicatorOutput};

/// Cache of pre-computed indicator values, keyed by `IndicatorConfig::cache_key()`.
pub type IndicatorCache = HashMap<String, IndicatorOutput>;

/// Cache of daily OHLC boundaries aligned to each bar.
#[derive(Debug)]
pub struct DailyOhlcCache {
    /// Open price of the first bar of the current day.
    pub daily_open: Vec<f64>,
    /// Running highest high of the current day up to this bar.
    pub daily_high: Vec<f64>,
    /// Running lowest low of the current day up to this bar.
    pub daily_low: Vec<f64>,
    /// Close price of the last bar of the previous day.
    pub daily_close: Vec<f64>,
}

/// Pre-compute daily OHLC boundaries from candle data.
pub fn compute_daily_ohlc(candles: &[Candle]) -> DailyOhlcCache {
    let len = candles.len();
    let mut daily_open = vec![f64::NAN; len];
    let mut daily_high = vec![f64::NAN; len];
    let mut daily_low = vec![f64::NAN; len];
    let mut daily_close = vec![f64::NAN; len];

    let mut prev_date = String::new();
    let mut day_open = f64::NAN;
    let mut day_high = f64::NEG_INFINITY;
    let mut day_low = f64::INFINITY;
    let mut prev_day_close = f64::NAN;

    for i in 0..len {
        let current_date = if candles[i].datetime.len() >= 10 {
            &candles[i].datetime[..10]
        } else {
            &candles[i].datetime
        };

        if current_date != prev_date {
            // New day — save previous day's close
            if i > 0 {
                prev_day_close = candles[i - 1].close;
            }
            day_open = candles[i].open;
            day_high = candles[i].high;
            day_low = candles[i].low;
            prev_date = current_date.to_string();
        } else {
            day_high = day_high.max(candles[i].high);
            day_low = day_low.min(candles[i].low);
        }

        daily_open[i] = day_open;
        daily_high[i] = day_high;
        daily_low[i] = day_low;
        daily_close[i] = prev_day_close;
    }

    DailyOhlcCache {
        daily_open,
        daily_high,
        daily_low,
        daily_close,
    }
}

// ── TimeCache ──

/// Pre-computed time/bar-derived values for each candle.
#[derive(Debug)]
pub struct TimeCache {
    pub current_bar: Vec<f64>,
    pub bar_time: Vec<f64>,
    pub bar_hour: Vec<f64>,
    pub bar_minute: Vec<f64>,
    pub bar_day_of_week: Vec<f64>,
    pub bar_month: Vec<f64>,
}

/// Tomohiko Sakamoto's day-of-week algorithm. Returns 0=Sun, 1=Mon, ..., 6=Sat.
fn day_of_week(mut y: i32, m: i32, d: i32) -> i32 {
    static T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7
}

/// Pre-compute all time-derived fields from candle datetimes.
/// Expects datetime format "YYYY-MM-DD HH:MM:SS..." (at least 16 chars).
pub fn compute_time_cache(candles: &[Candle]) -> TimeCache {
    let len = candles.len();
    let mut current_bar = Vec::with_capacity(len);
    let mut bar_time = Vec::with_capacity(len);
    let mut bar_hour = Vec::with_capacity(len);
    let mut bar_minute = Vec::with_capacity(len);
    let mut bar_day_of_week = Vec::with_capacity(len);
    let mut bar_month = Vec::with_capacity(len);

    for (i, candle) in candles.iter().enumerate() {
        let bytes = candle.datetime.as_bytes();

        // Parse YYYY-MM-DD HH:MM from fixed positions
        let year = if bytes.len() >= 4 {
            (bytes[0] - b'0') as i32 * 1000
                + (bytes[1] - b'0') as i32 * 100
                + (bytes[2] - b'0') as i32 * 10
                + (bytes[3] - b'0') as i32
        } else { 2000 };
        let month_val = if bytes.len() >= 7 {
            (bytes[5] - b'0') as i32 * 10 + (bytes[6] - b'0') as i32
        } else { 1 };
        let day = if bytes.len() >= 10 {
            (bytes[8] - b'0') as i32 * 10 + (bytes[9] - b'0') as i32
        } else { 1 };
        let hour = if bytes.len() >= 13 {
            (bytes[11] - b'0') as i32 * 10 + (bytes[12] - b'0') as i32
        } else { 0 };
        let min = if bytes.len() >= 16 {
            (bytes[14] - b'0') as i32 * 10 + (bytes[15] - b'0') as i32
        } else { 0 };

        current_bar.push(i as f64);
        bar_hour.push(hour as f64);
        bar_minute.push(min as f64);
        bar_time.push((hour * 60 + min) as f64);
        bar_month.push(month_val as f64);
        bar_day_of_week.push(day_of_week(year, month_val, day) as f64);
    }

    TimeCache {
        current_bar,
        bar_time,
        bar_hour,
        bar_minute,
        bar_day_of_week,
        bar_month,
    }
}

/// Check if a strategy uses any BarTime operands.
pub fn strategy_uses_time_fields(strategy: &Strategy) -> bool {
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter());
    for rule in all_rules {
        if rule.left_operand.operand_type == OperandType::BarTime
            || rule.right_operand.operand_type == OperandType::BarTime
        {
            return true;
        }
    }
    false
}

// ── CandlePatternCache ──

/// Pre-computed candle pattern detection results for each bar (1.0 = pattern found, 0.0 = not).
#[derive(Debug)]
pub struct CandlePatternCache {
    pub doji: Vec<f64>,
    pub hammer: Vec<f64>,
    pub shooting_star: Vec<f64>,
    pub bearish_engulfing: Vec<f64>,
    pub bullish_engulfing: Vec<f64>,
    pub dark_cloud: Vec<f64>,
    pub piercing_line: Vec<f64>,
}

/// Pre-compute all candle pattern detections in a single pass.
pub fn compute_candle_pattern_cache(candles: &[Candle]) -> CandlePatternCache {
    let len = candles.len();
    let mut doji = vec![0.0_f64; len];
    let mut hammer = vec![0.0_f64; len];
    let mut shooting_star = vec![0.0_f64; len];
    let mut bearish_engulfing = vec![0.0_f64; len];
    let mut bullish_engulfing = vec![0.0_f64; len];
    let mut dark_cloud = vec![0.0_f64; len];
    let mut piercing_line = vec![0.0_f64; len];

    for i in 0..len {
        let c = &candles[i];
        let body = (c.close - c.open).abs();
        let range = c.high - c.low;
        let upper_shadow = c.high - c.open.max(c.close);
        let lower_shadow = c.open.min(c.close) - c.low;

        // Doji: body ≤ 10% of range
        if range > 0.0 && body <= 0.1 * range {
            doji[i] = 1.0;
        }

        // Hammer: small body at top, long lower shadow ≥ 2× body, upper shadow ≤ body
        if body > 0.0 && lower_shadow >= 2.0 * body && upper_shadow <= body {
            hammer[i] = 1.0;
        }

        // Shooting Star: small body at bottom, long upper shadow ≥ 2× body, lower shadow ≤ body
        if body > 0.0 && upper_shadow >= 2.0 * body && lower_shadow <= body {
            shooting_star[i] = 1.0;
        }

        // Two-bar patterns require previous bar
        if i > 0 {
            let p = &candles[i - 1];
            let prev_body = (p.close - p.open).abs();
            let prev_bullish = p.close > p.open;
            let prev_bearish = p.close < p.open;
            let curr_bullish = c.close > c.open;
            let curr_bearish = c.close < c.open;
            let prev_mid = (p.open + p.close) / 2.0;

            // Bearish Engulfing: prev bullish, current bearish, current body engulfs prev body
            if prev_bullish && curr_bearish && prev_body > 0.0
                && c.open >= p.close && c.close <= p.open
            {
                bearish_engulfing[i] = 1.0;
            }

            // Bullish Engulfing: prev bearish, current bullish, current body engulfs prev body
            if prev_bearish && curr_bullish && prev_body > 0.0
                && c.open <= p.close && c.close >= p.open
            {
                bullish_engulfing[i] = 1.0;
            }

            // Dark Cloud: prev bullish, current opens above prev high, closes below prev midpoint
            if prev_bullish && curr_bearish && prev_body > 0.0
                && c.open > p.high && c.close < prev_mid && c.close > p.open
            {
                dark_cloud[i] = 1.0;
            }

            // Piercing Line: prev bearish, current opens below prev low, closes above prev midpoint
            if prev_bearish && curr_bullish && prev_body > 0.0
                && c.open < p.low && c.close > prev_mid && c.close < p.open
            {
                piercing_line[i] = 1.0;
            }
        }
    }

    CandlePatternCache {
        doji,
        hammer,
        shooting_star,
        bearish_engulfing,
        bullish_engulfing,
        dark_cloud,
        piercing_line,
    }
}

/// Check if a strategy uses any CandlePattern operands.
pub fn strategy_uses_candle_patterns(strategy: &Strategy) -> bool {
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter());
    for rule in all_rules {
        if rule.left_operand.operand_type == OperandType::CandlePattern
            || rule.right_operand.operand_type == OperandType::CandlePattern
        {
            return true;
        }
    }
    false
}

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
///
/// `time_offset`: added to `bar_index` when resolving BarTime operands.
/// Pass 1 from the executor so that time-based rules reference the *execution*
/// bar (bar\[i\]) while indicators/prices come from the *signal* bar (bar\[i-1\]).
/// Pass 0 in tests or when no shift is needed.
pub fn evaluate_rules(
    rules: &[Rule],
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
    time_offset: usize,
) -> bool {
    if rules.is_empty() {
        return false;
    }

    let mut result = evaluate_single_rule(&rules[0], bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);

    for i in 1..rules.len() {
        let prev_operator = rules[i - 1]
            .logical_operator
            .unwrap_or(LogicalOperator::And);
        let current = evaluate_single_rule(&rules[i], bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);

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
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
    time_offset: usize,
) -> bool {
    let left = resolve_operand(&rule.left_operand, bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
    let right = resolve_operand(&rule.right_operand, bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);

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
            let prev_left = resolve_operand(&rule.left_operand, bar_index - 1, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            let prev_right = resolve_operand(&rule.right_operand, bar_index - 1, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            if prev_left.is_nan() || prev_right.is_nan() {
                return false;
            }
            prev_left <= prev_right && left > right
        }
        Comparator::CrossBelow => {
            if bar_index == 0 {
                return false;
            }
            let prev_left = resolve_operand(&rule.left_operand, bar_index - 1, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            let prev_right = resolve_operand(&rule.right_operand, bar_index - 1, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            if prev_left.is_nan() || prev_right.is_nan() {
                return false;
            }
            prev_left >= prev_right && left < right
        }
    }
}

/// Resolve an operand's value at a specific bar index.
///
/// `time_offset` is added to `bar_index` for BarTime operands so that
/// time-based rules reference the execution bar while indicators use
/// the signal bar's data.
fn resolve_operand(
    operand: &Operand,
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
    time_offset: usize,
) -> f64 {
    // BarTime operands use bar_index + time_offset so they resolve to
    // the execution bar's time, not the data/signal bar's time.
    let base_index = if operand.operand_type == OperandType::BarTime {
        bar_index + time_offset
    } else {
        bar_index
    };

    let effective_index = if let Some(offset) = operand.offset {
        if base_index < offset {
            return f64::NAN;
        }
        base_index - offset
    } else {
        base_index
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
                Some(PriceField::DailyOpen) => daily_ohlc
                    .map(|d| d.daily_open[effective_index])
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyHigh) => daily_ohlc
                    .map(|d| d.daily_high[effective_index])
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyLow) => daily_ohlc
                    .map(|d| d.daily_low[effective_index])
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyClose) => daily_ohlc
                    .map(|d| d.daily_close[effective_index])
                    .unwrap_or(f64::NAN),
                None => candle.close,
            }
        }
        OperandType::Constant => operand.constant_value.unwrap_or(0.0),
        OperandType::BarTime => {
            if let Some(tc) = time_cache {
                let idx = effective_index;
                match operand.time_field {
                    Some(TimeField::CurrentBar) => tc.current_bar[idx],
                    Some(TimeField::BarTimeValue) | Some(TimeField::CurrentTime) => tc.bar_time[idx],
                    Some(TimeField::BarHour) | Some(TimeField::CurrentHour) => tc.bar_hour[idx],
                    Some(TimeField::BarMinute) | Some(TimeField::CurrentMinute) => tc.bar_minute[idx],
                    Some(TimeField::BarDayOfWeek) | Some(TimeField::CurrentDayOfWeek) => tc.bar_day_of_week[idx],
                    Some(TimeField::CurrentMonth) => tc.bar_month[idx],
                    None => f64::NAN,
                }
            } else {
                f64::NAN
            }
        }
        OperandType::CandlePattern => {
            if let Some(pc) = pattern_cache {
                let idx = effective_index;
                match operand.candle_pattern {
                    Some(CandlePatternType::Doji) => pc.doji[idx],
                    Some(CandlePatternType::Hammer) => pc.hammer[idx],
                    Some(CandlePatternType::ShootingStar) => pc.shooting_star[idx],
                    Some(CandlePatternType::BearishEngulfing) => pc.bearish_engulfing[idx],
                    Some(CandlePatternType::BullishEngulfing) => pc.bullish_engulfing[idx],
                    Some(CandlePatternType::DarkCloud) => pc.dark_cloud[idx],
                    Some(CandlePatternType::PiercingLine) => pc.piercing_line[idx],
                    None => f64::NAN,
                }
            } else {
                f64::NAN
            }
        }
    }
}

/// Get the appropriate value from an indicator output based on the output_field.
fn get_indicator_value(
    output: &IndicatorOutput,
    config: &IndicatorConfig,
    index: usize,
) -> f64 {
    // First check extra map (Ichimoku, Pivots, Fibonacci, etc.)
    if let Some(ref field) = config.output_field {
        if let Some(ref extra) = output.extra {
            if let Some(vals) = extra.get(field.as_str()) {
                return vals.get(index).copied().unwrap_or(f64::NAN);
            }
        }
    }

    match config.output_field.as_deref() {
        Some("signal") | Some("d") | Some("aroon_down") | Some("vi_minus")
        | Some("fractal_down") | Some("ha_open") => {
            // Secondary output: MACD signal, Stochastic %D, Aroon Down, Vortex VI-,
            // Fractal Down, Heiken Ashi Open
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
            // Bollinger/Keltner upper band
            output
                .secondary
                .as_ref()
                .and_then(|s| s.get(index).copied())
                .unwrap_or(f64::NAN)
        }
        Some("lower") => {
            // Bollinger/Keltner lower band
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
        // New indicators
        Aroon | Momentum | Vortex => config.params.period.unwrap_or(14) + 1,
        AwesomeOscillator => 34,
        BarRange | TrueRange | HeikenAshi | LaguerreRSI | Pivots => 1,
        BiggestRange | HighestInRange | LowestInRange | SmallestRange
        | BearsPower | BullsPower | Fibonacci | GannHiLo | HullMA
        | LinearRegression | StdDev | UlcerIndex => config.params.period.unwrap_or(14),
        DeMarker => config.params.period.unwrap_or(14) + 1,
        Fractal => 5,
        Ichimoku => {
            let slow = config.params.slow_period.unwrap_or(26);
            let senkou_b = config.params.signal_period.unwrap_or(52);
            senkou_b + slow
        }
        KeltnerChannel | SuperTrend => config.params.period.unwrap_or(14) + 1,
        Reflex => config.params.period.unwrap_or(14) + 2,
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
            time_field: None,
            candle_pattern: None,
            offset: None,
        }
    }

    fn constant_operand(value: f64) -> Operand {
        Operand {
            operand_type: OperandType::Constant,
            indicator: None,
            price_field: None,
            constant_value: Some(value),
            time_field: None,
            candle_pattern: None,
            offset: None,
        }
    }

    fn price_operand(field: PriceField) -> Operand {
        Operand {
            operand_type: OperandType::Price,
            indicator: None,
            price_field: Some(field),
            constant_value: None,
            time_field: None,
            candle_pattern: None,
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
        assert!(!evaluate_rules(&rules, 0, &cache, &candles, None, None, None, 0)); // 10 > 15 = false
        assert!(evaluate_rules(&rules, 1, &cache, &candles, None, None, None, 0)); // 20 > 15 = true
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
        assert!(evaluate_rules(&strategy.long_entry_rules, 3, &cache, &candles, None, None, None, 0));
        // At idx 4 → prev=14.0 > 13.0, so no cross
        assert!(!evaluate_rules(&strategy.long_entry_rules, 4, &cache, &candles, None, None, None, 0));
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
        assert!(evaluate_rules(&rules, 0, &cache, &candles, None, None, None, 0));
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
        assert!(evaluate_rules(&rules, 0, &cache, &candles, None, None, None, 0));
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
                time_field: None,
                candle_pattern: None,
                offset: Some(1),
            },
            logical_operator: None,
        }];
        let cache = IndicatorCache::new();
        // Bar 2: close=30 > close[1]=20 → true
        assert!(evaluate_rules(&rules, 2, &cache, &candles, None, None, None, 0));
        // Bar 0: offset=1 would be index -1 → NaN → false
        assert!(!evaluate_rules(&rules, 0, &cache, &candles, None, None, None, 0));
    }

    #[test]
    fn test_candle_pattern_doji() {
        // Create candles where bar 1 is a Doji (body <= 10% of range)
        let candles = vec![
            Candle {
                datetime: "2024-01-01 00:00".to_string(),
                open: 100.0, high: 102.0, low: 98.0, close: 101.0,
                volume: 1000.0, ..Default::default()
            },
            // Doji: open ≈ close, long wicks
            Candle {
                datetime: "2024-01-01 01:00".to_string(),
                open: 100.0, high: 105.0, low: 95.0, close: 100.05,
                volume: 1000.0, ..Default::default()
            },
            Candle {
                datetime: "2024-01-01 02:00".to_string(),
                open: 100.0, high: 102.0, low: 98.0, close: 101.0,
                volume: 1000.0, ..Default::default()
            },
        ];

        let pc = compute_candle_pattern_cache(&candles);
        // Bar 1 should be Doji: body=0.05, range=10.0, body/range=0.5% < 10%
        assert_eq!(pc.doji[1], 1.0);
        assert_eq!(pc.doji[0], 0.0);
        assert_eq!(pc.doji[2], 0.0);

        // Test rule evaluation: CandlePattern(Doji) == Constant(1)
        let rules = vec![Rule {
            id: "r1".to_string(),
            left_operand: Operand {
                operand_type: OperandType::CandlePattern,
                candle_pattern: Some(CandlePatternType::Doji),
                indicator: None, price_field: None, constant_value: None,
                time_field: None, offset: None,
            },
            comparator: Comparator::Equal,
            right_operand: constant_operand(1.0),
            logical_operator: None,
        }];
        let cache = IndicatorCache::new();
        // Bar 1 is Doji → should match
        assert!(evaluate_rules(&rules, 1, &cache, &candles, None, None, Some(&pc), 0));
        // Bar 0 is not Doji → should not match
        assert!(!evaluate_rules(&rules, 0, &cache, &candles, None, None, Some(&pc), 0));
    }

    #[test]
    fn test_candle_pattern_bullish_engulfing() {
        // Bar 0: bearish, Bar 1: bullish engulfing
        let candles = vec![
            Candle {
                datetime: "2024-01-01 00:00".to_string(),
                open: 102.0, high: 103.0, low: 99.0, close: 100.0, // bearish
                volume: 1000.0, ..Default::default()
            },
            Candle {
                datetime: "2024-01-01 01:00".to_string(),
                open: 99.0, high: 104.0, low: 98.0, close: 103.0, // bullish, engulfs prev
                volume: 1000.0, ..Default::default()
            },
        ];

        let pc = compute_candle_pattern_cache(&candles);
        // Bar 1: prev bearish, curr bullish, c.open(99) <= p.close(100), c.close(103) >= p.open(102) → engulfing
        assert_eq!(pc.bullish_engulfing[1], 1.0);
        assert_eq!(pc.bullish_engulfing[0], 0.0);
        assert_eq!(pc.bearish_engulfing[1], 0.0);

        let rules = vec![Rule {
            id: "r1".to_string(),
            left_operand: Operand {
                operand_type: OperandType::CandlePattern,
                candle_pattern: Some(CandlePatternType::BullishEngulfing),
                indicator: None, price_field: None, constant_value: None,
                time_field: None, offset: None,
            },
            comparator: Comparator::Equal,
            right_operand: constant_operand(1.0),
            logical_operator: None,
        }];
        let cache = IndicatorCache::new();
        assert!(evaluate_rules(&rules, 1, &cache, &candles, None, None, Some(&pc), 0));
        assert!(!evaluate_rules(&rules, 0, &cache, &candles, None, None, Some(&pc), 0));
    }
}
