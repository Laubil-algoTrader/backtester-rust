//! Streaming indicator evaluation for tick-mode backtesting.
//!
//! Enables MT5-compatible "Every Tick" entry detection: indicators are computed
//! on-the-fly for the in-progress bar as each tick arrives, so entry signals can
//! fire mid-bar (matching MT5's behavior) rather than waiting for bar close.
//!
//! # Design
//! - [`IndicatorStreamState`] holds the minimal tail state needed for O(1) updates.
//! - [`StreamingSingleValue`] is a lightweight (3×f64) alternative to `IndicatorOutput`
//!   used in the hot tick loop to avoid Vec allocations.
//! - [`StreamingStateMap`] stores states in a `Vec` (indexed by `key_index` HashMap)
//!   so `update_streaming_vals` iterates a packed Vec with no hash computation.
//! - [`build_streaming_state`] is called ONCE per bar before the tick sub-loop.
//! - [`update_streaming_vals`] is called on every tick to refresh the in-memory Vec.

use std::collections::HashMap;

use crate::models::candle::Candle;
use crate::models::strategy::{IndicatorType, OperandType, Strategy};

use super::strategy::IndicatorCache;

// ══════════════════════════════════════════════════════════════
// Lightweight per-tick value
// ══════════════════════════════════════════════════════════════

/// Lightweight streaming value for a single indicator at the current tick.
///
/// Replaces indexing into full `Vec<f64>` inside the hot tick loop,
/// eliminating all per-tick heap allocations.
#[derive(Debug, Clone, Copy)]
pub struct StreamingSingleValue {
    /// Primary output (e.g. SMA, EMA, RSI, MACD line, BB middle).
    pub primary: f64,
    /// Secondary output (e.g. MACD signal, BB upper).
    pub secondary: Option<f64>,
    /// Tertiary output (e.g. MACD histogram, BB lower).
    pub tertiary: Option<f64>,
}

/// Vec-based streaming values — one entry per indicator, indexed by `StreamingStateMap::key_index`.
///
/// Replaces `HashMap<String, StreamingSingleValue>` to eliminate hash computation in the tick loop.
pub type StreamingVals = Vec<StreamingSingleValue>;

// ══════════════════════════════════════════════════════════════
// Stream state per indicator
// ══════════════════════════════════════════════════════════════

/// Minimal tail state for computing the next in-progress bar's indicator value.
///
/// Each variant holds ONLY what is needed: O(1) for EMA/RSI/ATR/MACD,
/// O(period) for SMA and Bollinger Bands (rolling window).
/// All other indicators fall back to the last completed bar value.
#[derive(Debug, Clone)]
pub enum IndicatorStreamState {
    /// SMA(period): pre-computed sum of the last `period` closes.
    /// Adding `running_close` and dividing by `period` gives the streaming SMA in O(1).
    /// `window_sum` is NaN when there are fewer than `period` completed bars.
    Sma {
        period: usize,
        /// Sum of the last `period` closes (constant during the tick sub-loop).
        window_sum: f64,
    },

    /// EMA(period): only prev_ema needed — O(1) update.
    Ema {
        period: usize,
        prev_ema: f64,
    },

    /// RSI(period): Wilder smoothed averages + prev close — O(1) update.
    Rsi {
        period: usize,
        prev_avg_gain: f64,
        prev_avg_loss: f64,
        prev_close: f64,
    },

    /// MACD(fast, slow, signal): three EMA tails — O(1) update.
    Macd {
        fast_period: usize,
        slow_period: usize,
        signal_period: usize,
        prev_fast_ema: f64,
        prev_slow_ema: f64,
        prev_signal_ema: f64,
    },

    /// Bollinger Bands(period, std_dev): pre-computed window statistics.
    /// Using the computational formula `var = E[x²] - E[x]²` to avoid O(period) per tick.
    BollingerBands {
        period: usize,
        std_dev_mult: f64,
        /// Sum of the last `period` closes (constant during the tick sub-loop).
        window_sum: f64,
        /// Sum of squares of the last `period` closes (constant during tick sub-loop).
        window_sum_sq: f64,
    },

    /// ATR(period): Wilder smoothed ATR + prev bar OHLC — O(1) update.
    Atr {
        period: usize,
        prev_atr: f64,
        prev_close: f64,
    },

    /// Approximation: use the last completed bar value unchanged.
    /// Used for Stochastic, ADX, CCI, WilliamsR, ROC, ParabolicSAR, VWAP,
    /// and all other indicators whose streaming formula is complex.
    /// Error is bounded by one bar's price movement (negligible at tick scale).
    LastValue {
        primary: f64,
        secondary: Option<f64>,
        tertiary: Option<f64>,
    },
}

// ══════════════════════════════════════════════════════════════
// State map
// ══════════════════════════════════════════════════════════════

/// Pre-built streaming state for all entry-rule indicators in a strategy.
///
/// `states` is a packed `Vec` for cache-friendly iteration in `update_streaming_vals`.
/// `key_index` maps `IndicatorConfig::cache_key()` → Vec index for O(1) lookup in strategy evaluation.
pub struct StreamingStateMap {
    /// Packed Vec of stream states — iterate without hash overhead.
    pub states: Vec<IndicatorStreamState>,
    /// Maps indicator cache key → index into `states` / `StreamingVals`.
    pub key_index: HashMap<String, usize>,
}

// ══════════════════════════════════════════════════════════════
// Public API
// ══════════════════════════════════════════════════════════════

/// Build streaming state from the completed indicator cache and candle series.
///
/// Called ONCE per bar (before the tick sub-loop). O(period) per indicator at worst.
/// Only processes entry rule indicators — exit rules still fire at bar open.
pub fn build_streaming_state(
    strategy: &Strategy,
    cache: &IndicatorCache,
    candles: &[Candle],
    bar_index: usize, // index of last completed bar (= i-1 in executor loop)
) -> StreamingStateMap {
    let mut states: Vec<IndicatorStreamState> = Vec::new();
    let mut key_index: HashMap<String, usize> = HashMap::new();

    let entry_rules = strategy
        .long_entry_rules
        .iter()
        .chain(strategy.short_entry_rules.iter());

    for rule in entry_rules {
        for operand in [&rule.left_operand, &rule.right_operand] {
            if operand.operand_type != OperandType::Indicator {
                continue;
            }
            if let Some(ref config) = operand.indicator {
                let key = config.cache_key();
                if key_index.contains_key(&key) {
                    continue;
                }
                let state = match config.indicator_type {
                    IndicatorType::SMA => {
                        let period = config.params.period.unwrap_or(14);
                        let (window_sum, _) = extract_window_sums(candles, bar_index, period);
                        IndicatorStreamState::Sma { period, window_sum }
                    }

                    IndicatorType::EMA
                    | IndicatorType::HullMA
                    | IndicatorType::LinearRegression
                    | IndicatorType::LaguerreRSI => {
                        let period = config.params.period.unwrap_or(14);
                        let prev_ema = cache
                            .get(&key)
                            .and_then(|o| o.primary.get(bar_index).copied())
                            .unwrap_or(f64::NAN);
                        IndicatorStreamState::Ema { period, prev_ema }
                    }

                    IndicatorType::RSI => {
                        let period = config.params.period.unwrap_or(14);
                        let (avg_gain, avg_loss) =
                            extract_rsi_tail(candles, period, bar_index);
                        let prev_close = candles
                            .get(bar_index)
                            .map(|c| c.close)
                            .unwrap_or(f64::NAN);
                        IndicatorStreamState::Rsi {
                            period,
                            prev_avg_gain: avg_gain,
                            prev_avg_loss: avg_loss,
                            prev_close,
                        }
                    }

                    IndicatorType::MACD => {
                        let fast = config.params.fast_period.unwrap_or(12);
                        let slow = config.params.slow_period.unwrap_or(26);
                        let signal = config.params.signal_period.unwrap_or(9);
                        let prev_fast_ema = extract_ema_tail(candles, fast, bar_index);
                        let prev_slow_ema = extract_ema_tail(candles, slow, bar_index);
                        let prev_signal_ema = cache
                            .get(&key)
                            .and_then(|o| o.secondary.as_ref())
                            .and_then(|s| s.get(bar_index).copied())
                            .unwrap_or(f64::NAN);
                        IndicatorStreamState::Macd {
                            fast_period: fast,
                            slow_period: slow,
                            signal_period: signal,
                            prev_fast_ema,
                            prev_slow_ema,
                            prev_signal_ema,
                        }
                    }

                    IndicatorType::BollingerBands | IndicatorType::KeltnerChannel => {
                        let period = config.params.period.unwrap_or(20);
                        let std_dev_mult = config.params.std_dev.unwrap_or(2.0);
                        let (window_sum, window_sum_sq) = extract_window_sums(candles, bar_index, period);
                        IndicatorStreamState::BollingerBands {
                            period,
                            std_dev_mult,
                            window_sum,
                            window_sum_sq,
                        }
                    }

                    IndicatorType::ATR | IndicatorType::TrueRange => {
                        let period = config.params.period.unwrap_or(14);
                        let prev_atr = cache
                            .get(&key)
                            .and_then(|o| o.primary.get(bar_index).copied())
                            .unwrap_or(f64::NAN);
                        let prev_close = candles
                            .get(bar_index)
                            .map(|c| c.close)
                            .unwrap_or(f64::NAN);
                        IndicatorStreamState::Atr {
                            period,
                            prev_atr,
                            prev_close,
                        }
                    }

                    // All other indicators: use last completed bar value as approximation.
                    _ => {
                        let output = cache.get(&key);
                        let primary = output
                            .and_then(|o| o.primary.get(bar_index).copied())
                            .unwrap_or(f64::NAN);
                        let secondary = output
                            .and_then(|o| o.secondary.as_ref())
                            .and_then(|s| s.get(bar_index).copied());
                        let tertiary = output
                            .and_then(|o| o.tertiary.as_ref())
                            .and_then(|t| t.get(bar_index).copied());
                        IndicatorStreamState::LastValue {
                            primary,
                            secondary,
                            tertiary,
                        }
                    }
                };
                let idx = states.len();
                states.push(state);
                key_index.insert(key, idx);
            }
        }
    }

    StreamingStateMap { states, key_index }
}

/// Initialize the streaming values Vec (pre-allocated; updated in-place each tick).
///
/// Call once before the tick sub-loop. The returned Vec is indexed by `StreamingStateMap::key_index`.
#[inline]
pub fn init_streaming_vals(state: &StreamingStateMap) -> StreamingVals {
    vec![
        StreamingSingleValue {
            primary: f64::NAN,
            secondary: None,
            tertiary: None,
        };
        state.states.len()
    ]
}

/// Update the streaming values Vec in-place for the current tick.
///
/// Zero allocation — iterates a packed Vec with no hash computation in the hot path.
/// Call on every tick inside the tick sub-loop.
#[inline(always)]
pub fn update_streaming_vals(
    state: &StreamingStateMap,
    vals: &mut StreamingVals,
    running_high: f64,
    running_low: f64,
    running_close: f64,
) {
    debug_assert_eq!(vals.len(), state.states.len());
    for (i, stream_state) in state.states.iter().enumerate() {
        let sv = compute_streaming_value(stream_state, running_high, running_low, running_close);
        // SAFETY: `i` is in bounds because vals.len() == state.states.len() (asserted above).
        *unsafe { vals.get_unchecked_mut(i) } = sv;
    }
}

// ══════════════════════════════════════════════════════════════
// Core streaming computation
// ══════════════════════════════════════════════════════════════

/// Compute a streaming indicator value for the current tick's running OHLCV.
#[inline(always)]
pub fn compute_streaming_value(
    state: &IndicatorStreamState,
    running_high: f64,
    running_low: f64,
    running_close: f64,
) -> StreamingSingleValue {
    match state {
        IndicatorStreamState::Sma { period, window_sum } => {
            if window_sum.is_nan() || *period == 0 {
                return nan_sv();
            }
            // O(1): pre-computed window sum + current tick close
            StreamingSingleValue {
                primary: (window_sum + running_close) / *period as f64,
                secondary: None,
                tertiary: None,
            }
        }

        IndicatorStreamState::Ema { period, prev_ema } => {
            if prev_ema.is_nan() || *period == 0 {
                return nan_sv();
            }
            let mult = 2.0 / (*period as f64 + 1.0);
            StreamingSingleValue {
                primary: (running_close - prev_ema) * mult + prev_ema,
                secondary: None,
                tertiary: None,
            }
        }

        IndicatorStreamState::Rsi {
            period,
            prev_avg_gain,
            prev_avg_loss,
            prev_close,
        } => {
            if prev_avg_gain.is_nan() || prev_avg_loss.is_nan() || prev_close.is_nan() {
                return nan_sv();
            }
            let change = running_close - prev_close;
            let gain = if change > 0.0 { change } else { 0.0 };
            let loss = if change < 0.0 { -change } else { 0.0 };
            let pf = *period as f64;
            let new_gain = (prev_avg_gain * (pf - 1.0) + gain) / pf;
            let new_loss = (prev_avg_loss * (pf - 1.0) + loss) / pf;
            let rsi = if new_loss == 0.0 {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + new_gain / new_loss)
            };
            StreamingSingleValue {
                primary: rsi,
                secondary: None,
                tertiary: None,
            }
        }

        IndicatorStreamState::Macd {
            fast_period,
            slow_period,
            signal_period,
            prev_fast_ema,
            prev_slow_ema,
            prev_signal_ema,
        } => {
            if prev_fast_ema.is_nan()
                || prev_slow_ema.is_nan()
                || prev_signal_ema.is_nan()
            {
                return nan_sv();
            }
            let fast_mult = 2.0 / (*fast_period as f64 + 1.0);
            let slow_mult = 2.0 / (*slow_period as f64 + 1.0);
            let sig_mult = 2.0 / (*signal_period as f64 + 1.0);
            let s_fast = (running_close - prev_fast_ema) * fast_mult + prev_fast_ema;
            let s_slow = (running_close - prev_slow_ema) * slow_mult + prev_slow_ema;
            let macd_val = s_fast - s_slow;
            let signal_val = (macd_val - prev_signal_ema) * sig_mult + prev_signal_ema;
            let histogram = macd_val - signal_val;
            StreamingSingleValue {
                primary: macd_val,
                secondary: Some(signal_val),
                tertiary: Some(histogram),
            }
        }

        IndicatorStreamState::BollingerBands {
            period,
            std_dev_mult,
            window_sum,
            window_sum_sq,
        } => {
            if window_sum.is_nan() || *period == 0 {
                return nan_sv();
            }
            let n = *period as f64;
            // O(1): computational formula — var = E[x²] - E[x]²
            let sum = window_sum + running_close;
            let sum_sq = window_sum_sq + running_close * running_close;
            let mean = sum / n;
            // abs() for floating-point numerical stability (tiny negatives near zero)
            let variance = (sum_sq / n - mean * mean).abs();
            let std_dev = variance.sqrt();
            // primary=middle, secondary=upper, tertiary=lower
            StreamingSingleValue {
                primary: mean,
                secondary: Some(mean + std_dev_mult * std_dev),
                tertiary: Some(mean - std_dev_mult * std_dev),
            }
        }

        IndicatorStreamState::Atr {
            period,
            prev_atr,
            prev_close,
        } => {
            if prev_atr.is_nan() || prev_close.is_nan() {
                return nan_sv();
            }
            let tr = (running_high - running_low)
                .max((running_high - prev_close).abs())
                .max((running_low - prev_close).abs());
            let pf = *period as f64;
            StreamingSingleValue {
                primary: (prev_atr * (pf - 1.0) + tr) / pf,
                secondary: None,
                tertiary: None,
            }
        }

        IndicatorStreamState::LastValue {
            primary,
            secondary,
            tertiary,
        } => StreamingSingleValue {
            primary: *primary,
            secondary: *secondary,
            tertiary: *tertiary,
        },
    }
}

// ══════════════════════════════════════════════════════════════
// Private helpers
// ══════════════════════════════════════════════════════════════

#[inline(always)]
fn nan_sv() -> StreamingSingleValue {
    StreamingSingleValue {
        primary: f64::NAN,
        secondary: None,
        tertiary: None,
    }
}

/// Compute the sum and sum-of-squares of the last `period - 1` completed closes ending at `bar_index`.
///
/// The streaming SMA/BB for the in-progress bar is:
///   `(window_sum + running_close) / period`
/// which requires exactly `period - 1` completed closes in the window so that
/// adding `running_close` yields `period` values total.
///
/// Returns `(NaN, NaN)` when there are fewer than `period - 1` completed bars.
/// Called ONCE per bar — O(period) but never in the hot tick loop.
fn extract_window_sums(candles: &[Candle], bar_index: usize, period: usize) -> (f64, f64) {
    if period == 0 {
        return (f64::NAN, f64::NAN);
    }
    // period == 1: empty window (0 completed closes). SMA(1) = running_close / 1. ✓
    if period == 1 {
        return (0.0, 0.0);
    }
    // period >= 2: need period-1 completed closes: range [bar_index+2-period ..= bar_index]
    // Requires bar_index + 2 >= period to avoid usize underflow.
    if bar_index + 2 < period {
        return (f64::NAN, f64::NAN);
    }
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    for idx in (bar_index + 2 - period)..=bar_index {
        let c = candles[idx].close;
        sum += c;
        sum_sq += c * c;
    }
    (sum, sum_sq)
}

/// Replay RSI Wilder smoothing up to `bar_index` to extract (avg_gain, avg_loss).
/// O(n) but called only once per bar.
fn extract_rsi_tail(candles: &[Candle], period: usize, bar_index: usize) -> (f64, f64) {
    if period == 0 || bar_index < period || candles.len() <= period {
        return (f64::NAN, f64::NAN);
    }

    // Seed: simple average of first `period` changes
    let mut avg_gain = 0.0f64;
    let mut avg_loss = 0.0f64;
    for i in 1..=period {
        if i >= candles.len() {
            return (f64::NAN, f64::NAN);
        }
        let change = candles[i].close - candles[i - 1].close;
        if change > 0.0 {
            avg_gain += change;
        } else {
            avg_loss -= change;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    // Wilder smoothing up to bar_index
    for i in (period + 1)..=bar_index {
        if i >= candles.len() {
            return (f64::NAN, f64::NAN);
        }
        let change = candles[i].close - candles[i - 1].close;
        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };
        let pf = period as f64;
        avg_gain = (avg_gain * (pf - 1.0) + gain) / pf;
        avg_loss = (avg_loss * (pf - 1.0) + loss) / pf;
    }

    (avg_gain, avg_loss)
}

/// Compute EMA at `bar_index` by replaying from scratch. O(n).
/// Used to extract fast/slow EMA tail state for MACD streaming.
fn extract_ema_tail(candles: &[Candle], period: usize, bar_index: usize) -> f64 {
    if period == 0 || bar_index < period - 1 || candles.len() <= bar_index {
        return f64::NAN;
    }
    let mult = 2.0 / (period as f64 + 1.0);
    let mut ema_val: f64 =
        candles[..period].iter().map(|c| c.close).sum::<f64>() / period as f64;
    for i in period..=bar_index {
        ema_val = (candles[i].close - ema_val) * mult + ema_val;
    }
    ema_val
}
