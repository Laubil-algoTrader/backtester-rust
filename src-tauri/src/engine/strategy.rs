use std::sync::Arc;

use dashmap::DashMap;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::strategy::{
    ArithmeticOp, CandlePatternType, Comparator, IndicatorConfig, LogicalOperator, Operand,
    OperandType, PriceField, Rule, RuleGroup, Strategy, TimeField,
};

use super::indicators::{CandleSlices, compute_indicator_with_slices, IndicatorOutput};
use super::streaming::{StreamingStateMap, StreamingVals};

/// Cache of pre-computed indicator values, keyed by `IndicatorConfig::cache_key_hash()`.
/// Values are wrapped in `Arc` so every cache hit is a ~5 ns pointer clone instead of
/// a 800 KB–2.4 MB deep copy of `Vec<f64>` data.
pub type IndicatorCache = DashMap<u64, Arc<IndicatorOutput>>;

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

fn operand_uses_bar_time(op: &Operand) -> bool {
    if op.operand_type == OperandType::BarTime { return true; }
    if let Some(ref l) = op.compound_left  { if operand_uses_bar_time(l)  { return true; } }
    if let Some(ref r) = op.compound_right { if operand_uses_bar_time(r)  { return true; } }
    false
}

fn operand_uses_candle_pattern(op: &Operand) -> bool {
    if op.operand_type == OperandType::CandlePattern { return true; }
    if let Some(ref l) = op.compound_left  { if operand_uses_candle_pattern(l) { return true; } }
    if let Some(ref r) = op.compound_right { if operand_uses_candle_pattern(r) { return true; } }
    false
}

/// Check if a strategy uses any BarTime operands.
pub fn strategy_uses_time_fields(strategy: &Strategy) -> bool {
    let group_rules = strategy.long_entry_groups.iter()
        .chain(strategy.short_entry_groups.iter())
        .chain(strategy.long_exit_groups.iter())
        .chain(strategy.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter())
        .chain(group_rules);
    for rule in all_rules {
        if operand_uses_bar_time(&rule.left_operand) || operand_uses_bar_time(&rule.right_operand) {
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
    let group_rules = strategy.long_entry_groups.iter()
        .chain(strategy.short_entry_groups.iter())
        .chain(strategy.long_exit_groups.iter())
        .chain(strategy.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter())
        .chain(group_rules);
    for rule in all_rules {
        if operand_uses_candle_pattern(&rule.left_operand) || operand_uses_candle_pattern(&rule.right_operand) {
            return true;
        }
    }
    false
}

/// Pre-compute all indicators referenced in a strategy's rules.
///
/// OHLCV vectors are extracted from `candles` once and reused for every indicator,
/// avoiding O(N × K) redundant allocations when N candles and K indicators are present.
/// Returns a cache that can be queried during rule evaluation.
pub fn pre_compute_indicators(
    strategy: &Strategy,
    candles: &[Candle],
) -> Result<IndicatorCache, AppError> {
    let cache = IndicatorCache::new();
    let mut seen = std::collections::HashSet::new();

    // Extract OHLCV once for all indicator computations.
    let slices = CandleSlices::from_candles(candles);

    // Collect all indicator configs from flat rules and group rules
    let group_rules = strategy.long_entry_groups.iter()
        .chain(strategy.short_entry_groups.iter())
        .chain(strategy.long_exit_groups.iter())
        .chain(strategy.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter())
        .chain(group_rules);
    for rule in all_rules {
        collect_indicator_from_operand(&rule.left_operand, &mut seen, &cache, &slices, candles)?;
        collect_indicator_from_operand(&rule.right_operand, &mut seen, &cache, &slices, candles)?;
    }

    Ok(cache)
}

/// Pre-compute indicators with a cross-thread shared cache to avoid redundant calculations.
///
/// When the same indicator (type + params) appears in multiple optimization combinations,
/// this function checks the shared cache first before computing. This can give a 5-20×
/// speedup in Grid Search where most combinations change only one parameter at a time.
///
/// The `shared` cache is lock-free (DashMap) per indicator key so parallel rayon workers
/// mostly proceed without contention. Recurses into `OperandType::Compound` so indicators
/// nested inside arithmetic expressions are also pre-computed and cached.
pub fn pre_compute_indicators_with_shared_cache(
    strategy: &Strategy,
    candles: &[Candle],
    shared: &Arc<IndicatorCache>,
) -> Result<IndicatorCache, AppError> {
    let local_cache = IndicatorCache::new();
    let mut seen = std::collections::HashSet::new();

    // Lazy: only allocate CandleSlices on the first cache miss.
    // In steady state (after the persistent cache warms up in generation 1) every indicator
    // is already cached, so this allocation is skipped entirely — saving ~400 KB per call.
    let mut slices: Option<CandleSlices> = None;

    let group_rules_sc = strategy.long_entry_groups.iter()
        .chain(strategy.short_entry_groups.iter())
        .chain(strategy.long_exit_groups.iter())
        .chain(strategy.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    // collect into a Vec to avoid lifetime issues with chained iterators and the loop body
    let all_rules_vec: Vec<&Rule> = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter())
        .chain(group_rules_sc)
        .collect();

    for rule in all_rules_vec {
        for operand in [&rule.left_operand, &rule.right_operand] {
            pre_compute_operand_shared(operand, &mut seen, shared, &local_cache, &mut slices, candles)?;
        }
    }

    Ok(local_cache)
}

/// Recursive helper for [`pre_compute_indicators_with_shared_cache`].
/// Handles both top-level `Indicator` operands and indicators nested inside
/// `Compound` arithmetic expressions (e.g. `EMA(20) + ATR(14)`).
fn pre_compute_operand_shared(
    operand: &Operand,
    seen: &mut std::collections::HashSet<u64>,
    shared: &Arc<IndicatorCache>,
    local_cache: &IndicatorCache,
    slices: &mut Option<CandleSlices>,
    candles: &[Candle],
) -> Result<(), AppError> {
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                let key = config.cache_key_hash();
                if seen.insert(key) {
                    // Check shared DashMap cache first (lock-free read)
                    let from_shared = shared.get(&key).map(|v| Arc::clone(&*v));
                    let output: Arc<IndicatorOutput> = if let Some(cached) = from_shared {
                        // Arc clone: ~5 ns, no data copy
                        cached
                    } else {
                        // Cache miss: allocate CandleSlices lazily (only on first miss)
                        let s = slices.get_or_insert_with(|| CandleSlices::from_candles(candles));
                        let computed = Arc::new(compute_indicator_with_slices(config, s, candles)?);
                        // Store Arc in shared DashMap cache (no data copy, just pointer)
                        shared.insert(key, Arc::clone(&computed));
                        computed
                    };
                    local_cache.insert(key, output);
                }
            }
        }
        // Recurse into compound sub-operands so their indicators are also pre-computed.
        OperandType::Compound => {
            if let Some(ref left) = operand.compound_left {
                pre_compute_operand_shared(left, seen, shared, local_cache, slices, candles)?;
            }
            if let Some(ref right) = operand.compound_right {
                pre_compute_operand_shared(right, seen, shared, local_cache, slices, candles)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_indicator_from_operand(
    operand: &Operand,
    seen: &mut std::collections::HashSet<u64>,
    cache: &IndicatorCache,
    slices: &CandleSlices,
    candles: &[Candle],
) -> Result<(), AppError> {
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                let key = config.cache_key_hash();
                if seen.insert(key) {
                    let output = Arc::new(compute_indicator_with_slices(config, slices, candles)?);
                    cache.insert(key, output);
                }
            }
        }
        // Recurse into compound sub-operands so their indicators are also pre-computed.
        OperandType::Compound => {
            if let Some(ref left) = operand.compound_left {
                collect_indicator_from_operand(left, seen, cache, slices, candles)?;
            }
            if let Some(ref right) = operand.compound_right {
                collect_indicator_from_operand(right, seen, cache, slices, candles)?;
            }
        }
        _ => {}
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

        // Short-circuit: skip evaluation when result is already determined
        match prev_operator {
            LogicalOperator::And if !result => continue,
            LogicalOperator::Or if result => continue,
            _ => {}
        }

        let current = evaluate_single_rule(&rules[i], bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);

        match prev_operator {
            LogicalOperator::And => result = result && current,
            LogicalOperator::Or => result = result || current,
        }
    }

    result
}

/// Evaluate a single rule group: all rules combined via `g.internal` (AND = all pass, OR = any pass).
fn evaluate_group(
    g: &RuleGroup,
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
    time_offset: usize,
) -> bool {
    if g.rules.is_empty() {
        return false;
    }
    match g.internal {
        LogicalOperator::And => {
            for rule in &g.rules {
                if !evaluate_single_rule(rule, bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset) {
                    return false;
                }
            }
            true
        }
        LogicalOperator::Or => {
            for rule in &g.rules {
                if evaluate_single_rule(rule, bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset) {
                    return true;
                }
            }
            false
        }
    }
}

/// Evaluate a list of rule groups at a bar index.
///
/// Each group's rules are combined by the group's `internal` operator.
/// Adjacent groups are combined by the previous group's `join` operator.
/// An empty groups slice returns `false`.
pub fn evaluate_rule_groups(
    groups: &[RuleGroup],
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
    time_offset: usize,
) -> bool {
    if groups.is_empty() {
        return false;
    }

    let mut result = evaluate_group(&groups[0], bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);

    for i in 1..groups.len() {
        let join = groups[i - 1].join.unwrap_or(LogicalOperator::And);
        // Short-circuit
        match join {
            LogicalOperator::And if !result => continue,
            LogicalOperator::Or if result => continue,
            _ => {}
        }
        let current = evaluate_group(&groups[i], bar_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
        match join {
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
            prev_left < prev_right && left > right
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
            prev_left > prev_right && left < right
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
                // Use pre-computed hash (set by init_strategy_hashes before bar loop).
                // Fall back to live computation for any path that didn't call it.
                let key = if config.cached_hash != 0 { config.cached_hash } else { config.cache_key_hash() };
                if let Some(output) = cache.get(&key) {
                    get_indicator_value(&**output, config, effective_index)
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
        OperandType::Compound => {
            let left_op = match operand.compound_left.as_deref() {
                Some(l) => l,
                None => return f64::NAN,
            };
            let right_op = match operand.compound_right.as_deref() {
                Some(r) => r,
                None => return f64::NAN,
            };
            let l = resolve_operand(left_op, effective_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            let r = resolve_operand(right_op, effective_index, cache, candles, daily_ohlc, time_cache, pattern_cache, time_offset);
            if l.is_nan() || r.is_nan() {
                return f64::NAN;
            }
            match operand.compound_op {
                Some(ArithmeticOp::Add) => l + r,
                Some(ArithmeticOp::Sub) => l - r,
                Some(ArithmeticOp::Mul) => l * r,
                Some(ArithmeticOp::Div) => {
                    if r.abs() < f64::EPSILON { f64::NAN } else { l / r }
                }
                None => f64::NAN,
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

// ══════════════════════════════════════════════════════════════
// Streaming rule evaluation (tick-mode "Every Tick" entry)
// ══════════════════════════════════════════════════════════════

/// Evaluate entry rules with streaming indicator overrides for the in-progress bar.
///
/// Identical to [`evaluate_rules`] except that when an indicator or price operand
/// resolves to `bar_index` (the current in-progress bar), it uses the streaming
/// value computed from the latest tick instead of the completed-bar cache.
///
/// This enables MT5-compatible "Every Tick" entry detection: a crossover signal
/// fires on the tick where the streaming indicator first crosses the threshold,
/// not at the close of the next completed bar.
///
/// # Arguments
/// - `bar_index` — the in-progress bar index (= `i` in the executor loop).
/// - `streaming_vals` — per-indicator streaming values for `bar_index`, keyed by
///   `IndicatorConfig::cache_key()`. Updated every tick by [`streaming::update_streaming_vals`].
/// - `running_candle` — synthetic candle for `bar_index` with OHLCV updated to the
///   current tick (open = bar open, high/low = running extremes, close = current tick).
pub fn evaluate_rules_streaming(
    rules: &[Rule],
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    cross_prev: &[Option<(f64, f64)>],
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> bool {
    if rules.is_empty() {
        return false;
    }

    let mut result = evaluate_single_rule_streaming(
        &rules[0], 0, bar_index, cache, streaming_state, streaming_vals, cross_prev,
        candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );

    for i in 1..rules.len() {
        let prev_op = rules[i - 1].logical_operator.unwrap_or(LogicalOperator::And);
        match prev_op {
            LogicalOperator::And if !result => continue,
            LogicalOperator::Or if result => continue,
            _ => {}
        }
        let current = evaluate_single_rule_streaming(
            &rules[i], i, bar_index, cache, streaming_state, streaming_vals, cross_prev,
            candles, running_candle, daily_ohlc, time_cache, pattern_cache,
        );
        match prev_op {
            LogicalOperator::And => result = result && current,
            LogicalOperator::Or => result = result || current,
        }
    }

    result
}

fn evaluate_single_rule_streaming(
    rule: &Rule,
    rule_index: usize,
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    cross_prev: &[Option<(f64, f64)>],
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> bool {
    let left = resolve_operand_streaming(
        &rule.left_operand, bar_index, cache, streaming_state, streaming_vals,
        candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );
    let right = resolve_operand_streaming(
        &rule.right_operand, bar_index, cache, streaming_state, streaming_vals,
        candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );

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
            // prev values were pre-computed ONCE before the tick loop
            let Some((prev_left, prev_right)) = cross_prev.get(rule_index).and_then(|v| *v) else {
                return false;
            };
            prev_left < prev_right && left > right
        }
        Comparator::CrossBelow => {
            if bar_index == 0 {
                return false;
            }
            let Some((prev_left, prev_right)) = cross_prev.get(rule_index).and_then(|v| *v) else {
                return false;
            };
            prev_left > prev_right && left < right
        }
    }
}

/// Resolve an operand at `bar_index`, using streaming overrides when available.
///
/// For `effective_index == bar_index`:
/// - Indicator operands check `streaming_vals` first, fall back to cache.
/// - Price operands use `running_candle` (current tick's running OHLCV).
///
/// For `effective_index != bar_index`: identical to [`resolve_operand`].
fn resolve_operand_streaming(
    operand: &Operand,
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> f64 {
    // BarTime uses time_offset=0 in streaming context (no bar shift needed)
    let base_index = bar_index;

    let effective_index = if let Some(offset) = operand.offset {
        if base_index < offset {
            return f64::NAN;
        }
        base_index - offset
    } else {
        base_index
    };

    // For indices other than bar_index, fall back to the regular (completed-bar) resolver
    if effective_index != bar_index {
        return resolve_operand(
            operand, effective_index, cache, candles,
            daily_ohlc, time_cache, pattern_cache, 0,
        );
    }

    // effective_index == bar_index: use streaming overrides
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                let str_key = config.cache_key();
                // Vec lookup via key_index — no String hashing in hot path
                if let Some(&idx) = streaming_state.key_index.get(&str_key) {
                    if let Some(sv) = streaming_vals.get(idx) {
                        // Pick the right output based on output_field (mirrors get_indicator_value)
                        return match config.output_field.as_deref() {
                            Some("signal") | Some("d") => sv.secondary.unwrap_or(f64::NAN),
                            Some("histogram") => sv.tertiary.unwrap_or(f64::NAN),
                            Some("upper") => sv.secondary.unwrap_or(f64::NAN),
                            Some("lower") => sv.tertiary.unwrap_or(f64::NAN),
                            _ => sv.primary,
                        };
                    }
                }
                // No streaming override: fall back to cache (last completed bar).
                let hash_key = if config.cached_hash != 0 { config.cached_hash } else { config.cache_key_hash() };
                if let Some(output) = cache.get(&hash_key) {
                    get_indicator_value(&**output, config, effective_index)
                } else {
                    f64::NAN
                }
            } else {
                f64::NAN
            }
        }
        OperandType::Price => {
            // Use the running candle (current tick's OHLCV) instead of candles[bar_index]
            match operand.price_field {
                Some(PriceField::Open) => running_candle.open,
                Some(PriceField::High) => running_candle.high,
                Some(PriceField::Low) => running_candle.low,
                Some(PriceField::Close) | None => running_candle.close,
                // Daily fields are not meaningful for the in-progress bar; use prior day
                Some(PriceField::DailyOpen) => daily_ohlc
                    .and_then(|d| d.daily_open.get(effective_index).copied())
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyHigh) => daily_ohlc
                    .and_then(|d| d.daily_high.get(effective_index).copied())
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyLow) => daily_ohlc
                    .and_then(|d| d.daily_low.get(effective_index).copied())
                    .unwrap_or(f64::NAN),
                Some(PriceField::DailyClose) => daily_ohlc
                    .and_then(|d| d.daily_close.get(effective_index).copied())
                    .unwrap_or(f64::NAN),
            }
        }
        OperandType::Compound => {
            let left_op = match operand.compound_left.as_deref() {
                Some(l) => l,
                None => return f64::NAN,
            };
            let right_op = match operand.compound_right.as_deref() {
                Some(r) => r,
                None => return f64::NAN,
            };
            let l = resolve_operand_streaming(
                left_op, bar_index, cache, streaming_state, streaming_vals,
                candles, running_candle, daily_ohlc, time_cache, pattern_cache,
            );
            let r = resolve_operand_streaming(
                right_op, bar_index, cache, streaming_state, streaming_vals,
                candles, running_candle, daily_ohlc, time_cache, pattern_cache,
            );
            if l.is_nan() || r.is_nan() { return f64::NAN; }
            match operand.compound_op {
                Some(ArithmeticOp::Add) => l + r,
                Some(ArithmeticOp::Sub) => l - r,
                Some(ArithmeticOp::Mul) => l * r,
                Some(ArithmeticOp::Div) if r.abs() >= f64::EPSILON => l / r,
                _ => f64::NAN,
            }
        }
        // Constants, BarTime, and CandlePattern are not tick-sensitive — use regular resolver
        _ => resolve_operand(
            operand, bar_index, cache, candles,
            daily_ohlc, time_cache, pattern_cache, 0,
        ),
    }
}

/// Pre-compute CrossAbove/CrossBelow "previous bar" values for all entry rules.
///
/// Called ONCE per bar (before the tick sub-loop). Returns a Vec with one entry per rule;
/// `None` means the rule has no cross comparator or is at bar 0.
///
/// During the tick loop, `evaluate_single_rule_streaming` reads from this Vec instead
/// of calling `resolve_operand(bar_index - 1, ...)` on every tick.
pub fn precompute_cross_prev_vals(
    rules: &[Rule],
    bar_index: usize,
    cache: &IndicatorCache,
    candles: &[Candle],
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> Vec<Option<(f64, f64)>> {
    if bar_index == 0 {
        return vec![None; rules.len()];
    }
    let prev = bar_index - 1;
    rules.iter().map(|rule| {
        match rule.comparator {
            Comparator::CrossAbove | Comparator::CrossBelow => {
                let pl = resolve_operand(
                    &rule.left_operand, prev, cache, candles,
                    daily_ohlc, time_cache, pattern_cache, 0,
                );
                let pr = resolve_operand(
                    &rule.right_operand, prev, cache, candles,
                    daily_ohlc, time_cache, pattern_cache, 0,
                );
                if pl.is_nan() || pr.is_nan() { None } else { Some((pl, pr)) }
            }
            _ => None,
        }
    }).collect()
}

/// Calculate the maximum lookback period needed for a strategy's indicators.
pub fn max_lookback(strategy: &Strategy) -> usize {
    let mut max = 0usize;
    let all_rules = strategy.long_entry_rules.iter()
        .chain(strategy.short_entry_rules.iter())
        .chain(strategy.long_exit_rules.iter())
        .chain(strategy.short_exit_rules.iter());
    // Also include rules from groups
    let group_rules = strategy.long_entry_groups.iter()
        .chain(strategy.short_entry_groups.iter())
        .chain(strategy.long_exit_groups.iter())
        .chain(strategy.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    let mut has_cross = false;
    for rule in all_rules.chain(group_rules) {
        max = max.max(operand_lookback(&rule.left_operand));
        max = max.max(operand_lookback(&rule.right_operand));
        if matches!(rule.comparator, Comparator::CrossAbove | Comparator::CrossBelow) {
            has_cross = true;
        }
    }
    // Add 1 only when CrossAbove/CrossBelow is present (needs the previous bar)
    if has_cross { max + 1 } else { max }
}

/// Pre-compute `cached_hash` for every `IndicatorConfig` in a strategy.
/// Call once before `run_backtest_inner` starts the bar loop so that the hot
/// path can use `config.cached_hash` instead of re-hashing ~15 fields per bar.
pub fn init_strategy_hashes(strategy: &mut Strategy) {
    let flat_rules = strategy.long_entry_rules.iter_mut()
        .chain(strategy.short_entry_rules.iter_mut())
        .chain(strategy.long_exit_rules.iter_mut())
        .chain(strategy.short_exit_rules.iter_mut());
    let group_rules = strategy.long_entry_groups.iter_mut()
        .chain(strategy.short_entry_groups.iter_mut())
        .chain(strategy.long_exit_groups.iter_mut())
        .chain(strategy.short_exit_groups.iter_mut())
        .flat_map(|g| g.rules.iter_mut());
    for rule in flat_rules.chain(group_rules) {
        init_operand_hash(&mut rule.left_operand);
        init_operand_hash(&mut rule.right_operand);
    }
}

fn init_operand_hash(op: &mut Operand) {
    if let Some(ref mut config) = op.indicator {
        config.cached_hash = config.cache_key_hash();
    }
    if let Some(ref mut left) = op.compound_left {
        init_operand_hash(left);
    }
    if let Some(ref mut right) = op.compound_right {
        init_operand_hash(right);
    }
}

fn operand_lookback(operand: &Operand) -> usize {
    let base = match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                indicator_lookback(config)
            } else {
                0
            }
        }
        OperandType::Compound => {
            let l = operand.compound_left.as_deref().map(operand_lookback).unwrap_or(0);
            let r = operand.compound_right.as_deref().map(operand_lookback).unwrap_or(0);
            l.max(r)
        }
        _ => 0,
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

// ══════════════════════════════════════════════════════════════
// Fast (zero-allocation) rule evaluator for the tick loop
// ══════════════════════════════════════════════════════════════

/// Pre-resolved operand for zero-allocation evaluation in the hot tick loop.
///
/// Computed ONCE per bar from [`compile_rules_streaming`]. Replaces `cache_key()` String
/// generation + HashMap lookups that would otherwise run on every tick.
#[derive(Clone, Copy, Debug)]
pub enum FastOp {
    /// Streaming Vec index + output field (0=primary, 1=secondary, 2=tertiary).
    Stream(usize, u8),
    /// Price field from the running candle (Open/High/Low/Close only).
    Price(PriceField),
    /// Compile-time constant value.
    Const(f64),
    /// Needs full resolution (offset != 0, daily price field, BarTime, CandlePattern, etc.).
    Fallback,
}

/// Pre-compiled rule for zero-allocation tick evaluation.
#[derive(Clone, Debug)]
pub struct FastRule {
    pub left: FastOp,
    pub right: FastOp,
    pub comparator: Comparator,
    pub logical_op: Option<LogicalOperator>,
}

/// Pre-resolve a rule slice into `FastRule`s.
///
/// Called ONCE per bar (after [`streaming::build_streaming_state`]).
/// Pays the `cache_key()` String cost once per indicator per bar — not once per tick.
pub fn compile_rules_streaming(rules: &[Rule], streaming_state: &StreamingStateMap) -> Vec<FastRule> {
    rules.iter().map(|rule| FastRule {
        left: fast_op_for(&rule.left_operand, streaming_state),
        right: fast_op_for(&rule.right_operand, streaming_state),
        comparator: rule.comparator,
        logical_op: rule.logical_operator,
    }).collect()
}

fn fast_op_for(operand: &Operand, streaming_state: &StreamingStateMap) -> FastOp {
    // Operands with offsets need full per-tick resolution (bar_index - offset != bar_index)
    if operand.offset.is_some() {
        return FastOp::Fallback;
    }
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref config) = operand.indicator {
                // One-time cache_key() per operand per bar — not per tick
                let key = config.cache_key();
                if let Some(&idx) = streaming_state.key_index.get(&key) {
                    let field: u8 = match config.output_field.as_deref() {
                        Some("signal") | Some("d") | Some("upper") => 1,
                        Some("histogram") | Some("lower") => 2,
                        _ => 0,
                    };
                    return FastOp::Stream(idx, field);
                }
            }
            FastOp::Fallback
        }
        OperandType::Price => match operand.price_field {
            Some(pf @ PriceField::Open)
            | Some(pf @ PriceField::High)
            | Some(pf @ PriceField::Low)
            | Some(pf @ PriceField::Close) => FastOp::Price(pf),
            None => FastOp::Price(PriceField::Close),
            _ => FastOp::Fallback, // Daily fields handled by full fallback
        },
        OperandType::Constant => FastOp::Const(operand.constant_value.unwrap_or(0.0)),
        _ => FastOp::Fallback, // BarTime, CandlePattern
    }
}

/// Evaluate entry rules using pre-compiled [`FastRule`]s.
///
/// Zero-allocation per tick in the common case (all operands are `Stream`, `Price`, or `Const`).
/// Falls back to [`resolve_operand_streaming`] only for rare operands (offsets, daily fields, etc.).
#[inline(always)]
pub fn evaluate_rules_fast(
    fast_rules: &[FastRule],
    rules: &[Rule],
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    cross_prev: &[Option<(f64, f64)>],
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> bool {
    if fast_rules.is_empty() {
        return false;
    }
    let mut result = eval_fast_single(
        &fast_rules[0], &rules[0], 0, bar_index, cache, streaming_state, streaming_vals,
        cross_prev, candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );
    for i in 1..fast_rules.len() {
        let prev_op = fast_rules[i - 1].logical_op.unwrap_or(LogicalOperator::And);
        match prev_op {
            LogicalOperator::And if !result => continue,
            LogicalOperator::Or if result => continue,
            _ => {}
        }
        let current = eval_fast_single(
            &fast_rules[i], &rules[i], i, bar_index, cache, streaming_state, streaming_vals,
            cross_prev, candles, running_candle, daily_ohlc, time_cache, pattern_cache,
        );
        match prev_op {
            LogicalOperator::And => result = result && current,
            LogicalOperator::Or => result = result || current,
        }
    }
    result
}

#[inline(always)]
fn resolve_fast_op(
    op: FastOp,
    operand: &Operand,
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> f64 {
    match op {
        FastOp::Stream(idx, field) => {
            // SAFETY: idx < streaming_vals.len() is guaranteed because:
            // - `idx` comes from `streaming_state.key_index`, built in `build_streaming_state`
            //   where `idx = states.len()` before pushing, so idx < states.len().
            // - `streaming_vals` is initialized as `vec![..; state.states.len()]`.
            // - Both were built from the same `StreamingStateMap` in the same executor block.
            debug_assert!(idx < streaming_vals.len(), "FastOp::Stream idx out of bounds");
            let sv = unsafe { streaming_vals.get_unchecked(idx) };
            match field {
                0 => sv.primary,
                1 => sv.secondary.unwrap_or(f64::NAN),
                _ => sv.tertiary.unwrap_or(f64::NAN),
            }
        }
        FastOp::Const(v) => v,
        FastOp::Price(pf) => match pf {
            PriceField::Open  => running_candle.open,
            PriceField::High  => running_candle.high,
            PriceField::Low   => running_candle.low,
            PriceField::Close => running_candle.close,
            // Daily fields are FastOp::Fallback, so this branch is unreachable
            _ => f64::NAN,
        },
        FastOp::Fallback => resolve_operand_streaming(
            operand, bar_index, cache, streaming_state, streaming_vals,
            candles, running_candle, daily_ohlc, time_cache, pattern_cache,
        ),
    }
}

#[inline(always)]
fn eval_fast_single(
    fast: &FastRule,
    rule: &Rule,
    rule_index: usize,
    bar_index: usize,
    cache: &IndicatorCache,
    streaming_state: &StreamingStateMap,
    streaming_vals: &StreamingVals,
    cross_prev: &[Option<(f64, f64)>],
    candles: &[Candle],
    running_candle: &Candle,
    daily_ohlc: Option<&DailyOhlcCache>,
    time_cache: Option<&TimeCache>,
    pattern_cache: Option<&CandlePatternCache>,
) -> bool {
    let left = resolve_fast_op(
        fast.left, &rule.left_operand, bar_index, cache, streaming_state, streaming_vals,
        candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );
    let right = resolve_fast_op(
        fast.right, &rule.right_operand, bar_index, cache, streaming_state, streaming_vals,
        candles, running_candle, daily_ohlc, time_cache, pattern_cache,
    );
    if left.is_nan() || right.is_nan() {
        return false;
    }
    match fast.comparator {
        Comparator::GreaterThan   => left > right,
        Comparator::LessThan      => left < right,
        Comparator::GreaterOrEqual => left >= right,
        Comparator::LessOrEqual   => left <= right,
        Comparator::Equal         => (left - right).abs() < f64::EPSILON,
        Comparator::CrossAbove => {
            if bar_index == 0 { return false; }
            let Some((pl, pr)) = cross_prev.get(rule_index).and_then(|v| *v) else { return false; };
            pl < pr && left > right
        }
        Comparator::CrossBelow => {
            if bar_index == 0 { return false; }
            let Some((pl, pr)) = cross_prev.get(rule_index).and_then(|v| *v) else { return false; };
            pl > pr && left < right
        }
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
            compound_left: None,
            compound_op: None,
            compound_right: None,
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
            compound_left: None,
            compound_op: None,
            compound_right: None,
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
            long_entry_groups: vec![],
            short_entry_groups: vec![],
            long_exit_groups: vec![],
            short_exit_groups: vec![],
            position_sizing: PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: 1.0,
                decrease_factor: 0.9,
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
            entry_order: OrderType::Market,
            entry_order_offset_pips: 0.0,
            close_after_bars: None,
            move_sl_to_be: false,
            entry_order_indicator: None,
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
                compound_left: None,
                compound_op: None,
                compound_right: None,
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
                compound_left: None, compound_op: None, compound_right: None,
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
                compound_left: None, compound_op: None, compound_right: None,
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
