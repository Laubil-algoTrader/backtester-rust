use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::strategy::{IndicatorConfig, IndicatorType};

/// Output of an indicator computation. Multi-output indicators use secondary/tertiary.
#[derive(Debug, Clone)]
pub struct IndicatorOutput {
    /// Primary output (e.g. SMA values, RSI values, MACD line).
    pub primary: Vec<f64>,
    /// Secondary output (e.g. MACD signal, Stochastic %D, Bollinger upper).
    pub secondary: Option<Vec<f64>>,
    /// Tertiary output (e.g. MACD histogram, Bollinger lower).
    pub tertiary: Option<Vec<f64>>,
}

/// Compute an indicator from candle data based on its configuration.
pub fn compute_indicator(
    config: &IndicatorConfig,
    candles: &[Candle],
) -> Result<IndicatorOutput, AppError> {
    let len = candles.len();
    if len == 0 {
        return Err(AppError::InsufficientData {
            needed: 1,
            available: 0,
        });
    }

    let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
    let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
    let volume: Vec<f64> = candles.iter().map(|c| c.volume).collect();

    match config.indicator_type {
        IndicatorType::SMA => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: sma(&close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::EMA => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: ema(&close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::RSI => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: rsi(&close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::MACD => {
            let fast = config
                .params
                .fast_period
                .ok_or_else(|| AppError::InvalidIndicatorParams("MACD requires fast_period".into()))?;
            let slow = config
                .params
                .slow_period
                .ok_or_else(|| AppError::InvalidIndicatorParams("MACD requires slow_period".into()))?;
            let signal = config
                .params
                .signal_period
                .ok_or_else(|| AppError::InvalidIndicatorParams("MACD requires signal_period".into()))?;
            check_data_len(len, slow)?;
            let (macd_line, signal_line, histogram) = macd(&close, fast, slow, signal);
            Ok(IndicatorOutput {
                primary: macd_line,
                secondary: Some(signal_line),
                tertiary: Some(histogram),
            })
        }
        IndicatorType::BollingerBands => {
            let period = require_period(&config.params)?;
            let std_dev_mult = config.params.std_dev.unwrap_or(2.0);
            check_data_len(len, period)?;
            let (upper, middle, lower) = bollinger_bands(&close, period, std_dev_mult);
            Ok(IndicatorOutput {
                primary: middle,
                secondary: Some(upper),
                tertiary: Some(lower),
            })
        }
        IndicatorType::ATR => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: atr(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::Stochastic => {
            let k_period = config
                .params
                .k_period
                .ok_or_else(|| AppError::InvalidIndicatorParams("Stochastic requires k_period".into()))?;
            let d_period = config
                .params
                .d_period
                .ok_or_else(|| AppError::InvalidIndicatorParams("Stochastic requires d_period".into()))?;
            check_data_len(len, k_period)?;
            let (k, d) = stochastic(&high, &low, &close, k_period, d_period);
            Ok(IndicatorOutput {
                primary: k,
                secondary: Some(d),
                tertiary: None,
            })
        }
        IndicatorType::ADX => {
            let period = require_period(&config.params)?;
            check_data_len(len, period * 2 + 1)?;
            Ok(IndicatorOutput {
                primary: adx(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::CCI => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: cci(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::ROC => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: roc(&close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::WilliamsR => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: williams_r(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::ParabolicSAR => {
            let af = config.params.acceleration_factor.unwrap_or(0.02);
            let max_af = config.params.maximum_factor.unwrap_or(0.20);
            check_data_len(len, 2)?;
            Ok(IndicatorOutput {
                primary: parabolic_sar(&high, &low, af, max_af),
                secondary: None,
                tertiary: None,
            })
        }
        IndicatorType::VWAP => {
            check_data_len(len, 1)?;
            Ok(IndicatorOutput {
                primary: vwap(&high, &low, &close, &volume, candles),
                secondary: None,
                tertiary: None,
            })
        }
    }
}

// ── Helpers ──

fn require_period(
    params: &crate::models::strategy::IndicatorParams,
) -> Result<usize, AppError> {
    params
        .period
        .ok_or_else(|| AppError::InvalidIndicatorParams("period parameter is required".into()))
}

fn check_data_len(available: usize, needed: usize) -> Result<(), AppError> {
    if available < needed {
        return Err(AppError::InsufficientData { needed, available });
    }
    Ok(())
}

// ── SMA ──

/// Simple Moving Average. First `period-1` values are NaN.
pub fn sma(data: &[f64], period: usize) -> Vec<f64> {
    let len = data.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 || len < period {
        return result;
    }
    let mut sum: f64 = data[..period].iter().sum();
    result[period - 1] = sum / period as f64;
    for i in period..len {
        sum += data[i] - data[i - period];
        result[i] = sum / period as f64;
    }
    result
}

// ── EMA ──

/// Exponential Moving Average. First `period-1` values are NaN;
/// value at index `period-1` is seeded with SMA.
pub fn ema(data: &[f64], period: usize) -> Vec<f64> {
    let len = data.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 || len < period {
        return result;
    }
    let multiplier = 2.0 / (period as f64 + 1.0);
    // Seed with SMA
    let seed: f64 = data[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = seed;
    for i in period..len {
        result[i] = (data[i] - result[i - 1]) * multiplier + result[i - 1];
    }
    result
}

/// EMA computed on a pre-computed slice (e.g., for signal line on MACD values).
fn ema_on_slice(data: &[f64], period: usize) -> Vec<f64> {
    let len = data.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 || len < period {
        return result;
    }
    // Find first non-NaN window of `period` consecutive values for seed
    let mut start = None;
    for i in 0..=len - period {
        if data[i..i + period].iter().all(|v| !v.is_nan()) {
            start = Some(i);
            break;
        }
    }
    let start = match start {
        Some(s) => s,
        None => return result,
    };
    let multiplier = 2.0 / (period as f64 + 1.0);
    let seed: f64 = data[start..start + period].iter().sum::<f64>() / period as f64;
    result[start + period - 1] = seed;
    for i in (start + period)..len {
        if data[i].is_nan() {
            continue;
        }
        result[i] = (data[i] - result[i - 1]) * multiplier + result[i - 1];
    }
    result
}

// ── RSI ──

/// Relative Strength Index. First `period` values are NaN.
pub fn rsi(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 || len < period + 1 {
        return result;
    }

    let mut gains = vec![0.0f64; len];
    let mut losses = vec![0.0f64; len];

    for i in 1..len {
        let change = close[i] - close[i - 1];
        if change > 0.0 {
            gains[i] = change;
        } else {
            losses[i] = -change;
        }
    }

    // First average: simple average of first `period` changes
    let mut avg_gain: f64 = gains[1..=period].iter().sum::<f64>() / period as f64;
    let mut avg_loss: f64 = losses[1..=period].iter().sum::<f64>() / period as f64;

    result[period] = if avg_loss == 0.0 {
        100.0
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    };

    // Smoothed averages
    for i in (period + 1)..len {
        avg_gain = (avg_gain * (period as f64 - 1.0) + gains[i]) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + losses[i]) / period as f64;
        result[i] = if avg_loss == 0.0 {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
        };
    }
    result
}

// ── MACD ──

/// MACD: returns (macd_line, signal_line, histogram).
pub fn macd(
    close: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let len = close.len();
    let fast_ema = ema(close, fast_period);
    let slow_ema = ema(close, slow_period);

    let mut macd_line = vec![f64::NAN; len];
    for i in 0..len {
        if !fast_ema[i].is_nan() && !slow_ema[i].is_nan() {
            macd_line[i] = fast_ema[i] - slow_ema[i];
        }
    }

    let signal_line = ema_on_slice(&macd_line, signal_period);

    let mut histogram = vec![f64::NAN; len];
    for i in 0..len {
        if !macd_line[i].is_nan() && !signal_line[i].is_nan() {
            histogram[i] = macd_line[i] - signal_line[i];
        }
    }

    (macd_line, signal_line, histogram)
}

// ── Bollinger Bands ──

/// Bollinger Bands: returns (upper, middle, lower).
pub fn bollinger_bands(
    close: &[f64],
    period: usize,
    std_dev_mult: f64,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let len = close.len();
    let middle = sma(close, period);
    let mut upper = vec![f64::NAN; len];
    let mut lower = vec![f64::NAN; len];

    for i in (period - 1)..len {
        let window = &close[i + 1 - period..=i];
        let mean = middle[i];
        let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
        let std_dev = variance.sqrt();
        upper[i] = mean + std_dev_mult * std_dev;
        lower[i] = mean - std_dev_mult * std_dev;
    }

    (upper, middle, lower)
}

// ── ATR ──

/// Average True Range.
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut tr = vec![0.0f64; len];

    // First TR is just high - low
    tr[0] = high[0] - low[0];
    for i in 1..len {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    // ATR is SMA of TR (using Wilder's smoothing: first value is SMA, then smoothed)
    let mut result = vec![f64::NAN; len];
    if len < period {
        return result;
    }
    let mut atr_val: f64 = tr[..period].iter().sum::<f64>() / period as f64;
    result[period - 1] = atr_val;
    for i in period..len {
        atr_val = (atr_val * (period as f64 - 1.0) + tr[i]) / period as f64;
        result[i] = atr_val;
    }
    result
}

// ── Stochastic ──

/// Stochastic oscillator: returns (%K, %D).
pub fn stochastic(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    k_period: usize,
    d_period: usize,
) -> (Vec<f64>, Vec<f64>) {
    let len = high.len();
    let mut k = vec![f64::NAN; len];

    for i in (k_period - 1)..len {
        let window_high = &high[i + 1 - k_period..=i];
        let window_low = &low[i + 1 - k_period..=i];
        let highest = window_high.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lowest = window_low.iter().cloned().fold(f64::INFINITY, f64::min);
        let range = highest - lowest;
        k[i] = if range == 0.0 {
            50.0
        } else {
            (close[i] - lowest) / range * 100.0
        };
    }

    let d = sma_on_slice(&k, d_period);
    (k, d)
}

/// SMA computed on a slice that may contain NaN values.
fn sma_on_slice(data: &[f64], period: usize) -> Vec<f64> {
    let len = data.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 {
        return result;
    }
    for i in 0..len {
        if i + 1 < period {
            continue;
        }
        let window = &data[i + 1 - period..=i];
        if window.iter().all(|v| !v.is_nan()) {
            result[i] = window.iter().sum::<f64>() / period as f64;
        }
    }
    result
}

// ── ADX ──

/// Average Directional Index.
pub fn adx(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    if len < period * 2 + 1 {
        return result;
    }

    // True Range
    let mut tr = vec![0.0f64; len];
    let mut plus_dm = vec![0.0f64; len];
    let mut minus_dm = vec![0.0f64; len];

    for i in 1..len {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);

        let up_move = high[i] - high[i - 1];
        let down_move = low[i - 1] - low[i];

        plus_dm[i] = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        minus_dm[i] = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };
    }

    // Smoothed sums (Wilder's smoothing)
    let mut smooth_tr: f64 = tr[1..=period].iter().sum();
    let mut smooth_plus_dm: f64 = plus_dm[1..=period].iter().sum();
    let mut smooth_minus_dm: f64 = minus_dm[1..=period].iter().sum();

    let mut dx_values = vec![f64::NAN; len];

    for i in period..len {
        if i > period {
            smooth_tr = smooth_tr - smooth_tr / period as f64 + tr[i];
            smooth_plus_dm = smooth_plus_dm - smooth_plus_dm / period as f64 + plus_dm[i];
            smooth_minus_dm = smooth_minus_dm - smooth_minus_dm / period as f64 + minus_dm[i];
        }

        let plus_di = if smooth_tr == 0.0 {
            0.0
        } else {
            100.0 * smooth_plus_dm / smooth_tr
        };
        let minus_di = if smooth_tr == 0.0 {
            0.0
        } else {
            100.0 * smooth_minus_dm / smooth_tr
        };

        let di_sum = plus_di + minus_di;
        dx_values[i] = if di_sum == 0.0 {
            0.0
        } else {
            100.0 * (plus_di - minus_di).abs() / di_sum
        };
    }

    // ADX = smoothed DX over `period`
    let adx_start = period * 2 - 1;
    if adx_start >= len {
        return result;
    }
    let mut adx_val: f64 =
        dx_values[period..=adx_start].iter().filter(|v| !v.is_nan()).sum::<f64>() / period as f64;
    result[adx_start] = adx_val;

    for i in (adx_start + 1)..len {
        if dx_values[i].is_nan() {
            continue;
        }
        adx_val = (adx_val * (period as f64 - 1.0) + dx_values[i]) / period as f64;
        result[i] = adx_val;
    }
    result
}

// ── CCI ──

/// Commodity Channel Index.
pub fn cci(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];

    // Typical price
    let tp: Vec<f64> = (0..len).map(|i| (high[i] + low[i] + close[i]) / 3.0).collect();

    for i in (period - 1)..len {
        let window = &tp[i + 1 - period..=i];
        let mean = window.iter().sum::<f64>() / period as f64;
        let mean_dev = window.iter().map(|v| (v - mean).abs()).sum::<f64>() / period as f64;
        result[i] = if mean_dev == 0.0 {
            0.0
        } else {
            (tp[i] - mean) / (0.015 * mean_dev)
        };
    }
    result
}

// ── ROC ──

/// Rate of Change (percentage).
pub fn roc(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    for i in period..len {
        if close[i - period] != 0.0 {
            result[i] = (close[i] - close[i - period]) / close[i - period] * 100.0;
        }
    }
    result
}

// ── Williams %R ──

/// Williams %R.
pub fn williams_r(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let window_high = &high[i + 1 - period..=i];
        let window_low = &low[i + 1 - period..=i];
        let highest = window_high.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lowest = window_low.iter().cloned().fold(f64::INFINITY, f64::min);
        let range = highest - lowest;
        result[i] = if range == 0.0 {
            -50.0
        } else {
            (highest - close[i]) / range * -100.0
        };
    }
    result
}

// ── Parabolic SAR ──

/// Parabolic SAR.
pub fn parabolic_sar(
    high: &[f64],
    low: &[f64],
    acceleration_start: f64,
    acceleration_max: f64,
) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    if len < 2 {
        return result;
    }

    let mut is_long = high[1] > high[0];
    let mut af = acceleration_start;
    let mut ep = if is_long { high[0] } else { low[0] };
    let mut sar = if is_long { low[0] } else { high[0] };

    result[0] = sar;

    for i in 1..len {
        let prev_sar = sar;

        // Update SAR
        sar = prev_sar + af * (ep - prev_sar);

        if is_long {
            // SAR must not be above the two previous lows
            if i >= 2 {
                sar = sar.min(low[i - 1]).min(low[i - 2]);
            } else {
                sar = sar.min(low[i - 1]);
            }

            // Check for reversal
            if low[i] < sar {
                is_long = false;
                sar = ep;
                ep = low[i];
                af = acceleration_start;
            } else {
                if high[i] > ep {
                    ep = high[i];
                    af = (af + acceleration_start).min(acceleration_max);
                }
            }
        } else {
            // SAR must not be below the two previous highs
            if i >= 2 {
                sar = sar.max(high[i - 1]).max(high[i - 2]);
            } else {
                sar = sar.max(high[i - 1]);
            }

            // Check for reversal
            if high[i] > sar {
                is_long = true;
                sar = ep;
                ep = high[i];
                af = acceleration_start;
            } else {
                if low[i] < ep {
                    ep = low[i];
                    af = (af + acceleration_start).min(acceleration_max);
                }
            }
        }

        result[i] = sar;
    }
    result
}

// ── VWAP ──

/// Volume Weighted Average Price, reset daily.
pub fn vwap(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    candles: &[Candle],
) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    let mut cum_vol = 0.0f64;
    let mut cum_tp_vol = 0.0f64;
    let mut prev_date = String::new();

    for i in 0..len {
        // Extract date portion (first 10 chars: "YYYY-MM-DD")
        let current_date = if candles[i].datetime.len() >= 10 {
            &candles[i].datetime[..10]
        } else {
            &candles[i].datetime
        };

        // Reset on new day
        if current_date != prev_date {
            cum_vol = 0.0;
            cum_tp_vol = 0.0;
            prev_date = current_date.to_string();
        }

        let tp = (high[i] + low[i] + close[i]) / 3.0;
        cum_tp_vol += tp * volume[i];
        cum_vol += volume[i];

        result[i] = if cum_vol == 0.0 {
            tp
        } else {
            cum_tp_vol / cum_vol
        };
    }
    result
}

// ══════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, epsilon: f64) -> bool {
        if a.is_nan() && b.is_nan() {
            return true;
        }
        (a - b).abs() < epsilon
    }

    fn assert_approx(actual: f64, expected: f64, epsilon: f64, msg: &str) {
        assert!(
            approx_eq(actual, expected, epsilon),
            "{}: expected {}, got {}",
            msg,
            expected,
            actual
        );
    }

    #[test]
    fn test_sma_basic() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let result = sma(&data, 3);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert_approx(result[2], 2.0, 1e-10, "SMA[2]");
        assert_approx(result[3], 3.0, 1e-10, "SMA[3]");
        assert_approx(result[9], 9.0, 1e-10, "SMA[9]");
    }

    #[test]
    fn test_ema_basic() {
        let data = vec![22.27, 22.19, 22.08, 22.17, 22.18, 22.13, 22.23, 22.43, 22.24, 22.29];
        let result = ema(&data, 5);
        assert!(result[3].is_nan());
        // EMA(5) seed at index 4 = SMA of first 5
        let seed = (22.27 + 22.19 + 22.08 + 22.17 + 22.18) / 5.0;
        assert_approx(result[4], seed, 1e-10, "EMA seed");
        // Subsequent values use multiplier 2/(5+1) = 1/3
        assert!(!result[5].is_nan());
    }

    #[test]
    fn test_rsi_basic() {
        // Use a known sequence
        let data = vec![
            44.0, 44.34, 44.09, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08,
            45.89, 46.03, 45.61, 46.28, 46.28, 46.00, 46.03, 46.41, 46.22, 45.64,
        ];
        let result = rsi(&data, 14);
        // First 14 values should be NaN
        for i in 0..14 {
            assert!(result[i].is_nan(), "RSI[{}] should be NaN", i);
        }
        // RSI(14) at index 14 should be around 70
        assert!(result[14] > 50.0 && result[14] < 90.0, "RSI[14] = {} not in expected range", result[14]);
    }

    #[test]
    fn test_macd_basic() {
        let data: Vec<f64> = (1..=50).map(|i| 100.0 + (i as f64) * 0.5).collect();
        let (macd_line, signal, hist) = macd(&data, 12, 26, 9);
        assert_eq!(macd_line.len(), 50);
        // MACD line should have valid values starting from index 25 (slow period - 1)
        assert!(macd_line[25].is_finite());
        // For an uptrend, MACD should be positive
        assert!(macd_line[49] > 0.0, "MACD should be positive in uptrend");
        // Signal should lag behind
        assert!(signal[49].is_finite());
        // Histogram = MACD - signal
        if hist[49].is_finite() && macd_line[49].is_finite() && signal[49].is_finite() {
            assert_approx(hist[49], macd_line[49] - signal[49], 1e-10, "Histogram");
        }
    }

    #[test]
    fn test_bollinger_bands_basic() {
        let data = vec![20.0; 20]; // Constant price
        let (upper, middle, lower) = bollinger_bands(&data, 20, 2.0);
        // For constant data, std dev = 0, so upper = middle = lower
        assert_approx(middle[19], 20.0, 1e-10, "BB middle");
        assert_approx(upper[19], 20.0, 1e-10, "BB upper (no volatility)");
        assert_approx(lower[19], 20.0, 1e-10, "BB lower (no volatility)");
    }

    #[test]
    fn test_atr_basic() {
        let high = vec![48.70, 48.72, 48.90, 48.87, 48.82];
        let low = vec![47.79, 48.14, 48.39, 48.37, 48.24];
        let close = vec![48.16, 48.61, 48.75, 48.63, 48.74];
        let result = atr(&high, &low, &close, 3);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert!(result[2].is_finite(), "ATR[2] should be finite");
        assert!(result[2] > 0.0, "ATR should be positive");
    }

    #[test]
    fn test_stochastic_basic() {
        let high = vec![130.0, 132.0, 131.0, 133.0, 135.0, 134.0, 136.0, 138.0, 137.0, 139.0];
        let low = vec![126.0, 128.0, 127.0, 129.0, 131.0, 130.0, 132.0, 134.0, 133.0, 135.0];
        let close = vec![128.0, 131.0, 129.0, 132.0, 134.0, 132.0, 135.0, 137.0, 135.0, 138.0];
        let (k, d) = stochastic(&high, &low, &close, 5, 3);
        // %K should be valid from index 4 onward
        assert!(k[4].is_finite());
        assert!(k[4] >= 0.0 && k[4] <= 100.0, "%K should be 0-100");
        // %D should be valid from index 6 onward (k_period-1 + d_period-1)
        assert!(d[6].is_finite());
    }

    #[test]
    fn test_cci_basic() {
        let high = vec![25.0, 25.5, 26.0, 25.5, 25.0, 26.0, 27.0, 26.5, 26.0, 25.5];
        let low = vec![24.0, 24.5, 25.0, 24.5, 24.0, 25.0, 26.0, 25.5, 25.0, 24.5];
        let close = vec![24.5, 25.0, 25.5, 25.0, 24.5, 25.5, 26.5, 26.0, 25.5, 25.0];
        let result = cci(&high, &low, &close, 5);
        assert!(result[3].is_nan());
        assert!(result[4].is_finite(), "CCI[4] should be finite");
    }

    #[test]
    fn test_roc_basic() {
        let data = vec![10.0, 11.0, 12.0, 11.0, 13.0];
        let result = roc(&data, 2);
        assert!(result[0].is_nan());
        assert!(result[1].is_nan());
        assert_approx(result[2], 20.0, 1e-10, "ROC[2]"); // (12-10)/10*100
        assert_approx(result[3], 0.0, 1e-10, "ROC[3]"); // (11-11)/11*100
        assert_approx(result[4], (13.0 - 12.0) / 12.0 * 100.0, 1e-10, "ROC[4]"); // (13-12)/12*100
    }

    #[test]
    fn test_roc_values() {
        let data = vec![100.0, 105.0, 110.0, 108.0, 115.0];
        let result = roc(&data, 1);
        assert_approx(result[1], 5.0, 1e-10, "ROC[1]"); // (105-100)/100*100
        assert_approx(result[2], 100.0 * (110.0 - 105.0) / 105.0, 1e-10, "ROC[2]");
    }

    #[test]
    fn test_williams_r_basic() {
        let high = vec![130.0, 132.0, 131.0, 133.0, 135.0];
        let low = vec![126.0, 128.0, 127.0, 129.0, 131.0];
        let close = vec![128.0, 131.0, 129.0, 132.0, 134.0];
        let result = williams_r(&high, &low, &close, 5);
        // At index 4: highest=135, lowest=126, range=9, WR = (135-134)/9 * -100 = -11.11
        assert_approx(result[4], (135.0 - 134.0) / 9.0 * -100.0, 1e-10, "WilliamsR[4]");
    }

    #[test]
    fn test_parabolic_sar_basic() {
        let high = vec![35.0, 35.5, 36.0, 36.5, 37.0, 37.5, 38.0, 37.5, 37.0, 36.5];
        let low = vec![34.0, 34.5, 35.0, 35.5, 36.0, 36.5, 37.0, 36.5, 36.0, 35.5];
        let result = parabolic_sar(&high, &low, 0.02, 0.20);
        assert_eq!(result.len(), 10);
        assert!(result[0].is_finite());
        assert!(result[9].is_finite());
    }

    #[test]
    fn test_adx_basic() {
        // Simple trending data
        let high: Vec<f64> = (0..30).map(|i| 50.0 + i as f64 * 0.5).collect();
        let low: Vec<f64> = (0..30).map(|i| 49.0 + i as f64 * 0.5).collect();
        let close: Vec<f64> = (0..30).map(|i| 49.5 + i as f64 * 0.5).collect();
        let result = adx(&high, &low, &close, 7);
        // ADX should have values from index 13 (period*2-1 = 13) onward
        assert!(result[13].is_finite(), "ADX[13] should be finite");
        // In a steady uptrend, ADX should be relatively high
        assert!(result[29] > 0.0, "ADX should be positive");
    }

    #[test]
    fn test_vwap_basic() {
        let candles = vec![
            Candle { datetime: "2024-01-01 09:00".to_string(), open: 100.0, high: 102.0, low: 99.0, close: 101.0, volume: 1000.0, ..Default::default() },
            Candle { datetime: "2024-01-01 10:00".to_string(), open: 101.0, high: 103.0, low: 100.0, close: 102.0, volume: 1500.0, ..Default::default() },
            Candle { datetime: "2024-01-02 09:00".to_string(), open: 102.0, high: 104.0, low: 101.0, close: 103.0, volume: 2000.0, ..Default::default() },
        ];
        let high: Vec<f64> = candles.iter().map(|c| c.high).collect();
        let low: Vec<f64> = candles.iter().map(|c| c.low).collect();
        let close: Vec<f64> = candles.iter().map(|c| c.close).collect();
        let volume: Vec<f64> = candles.iter().map(|c| c.volume).collect();
        let result = vwap(&high, &low, &close, &volume, &candles);
        // First bar: TP = (102+99+101)/3 = 100.666..., VWAP = TP (first bar)
        let tp0 = (102.0 + 99.0 + 101.0) / 3.0;
        assert_approx(result[0], tp0, 1e-10, "VWAP[0]");
        // Third bar is a new day — should reset
        let tp2 = (104.0 + 101.0 + 103.0) / 3.0;
        assert_approx(result[2], tp2, 1e-10, "VWAP[2] (new day reset)");
    }

    #[test]
    fn test_compute_indicator_dispatches() {
        let candles: Vec<Candle> = (0..30)
            .map(|i| Candle {
                datetime: format!("2024-01-{:02} 00:00", (i % 28) + 1),
                open: 100.0 + i as f64,
                high: 101.0 + i as f64,
                low: 99.0 + i as f64,
                close: 100.5 + i as f64,
                volume: 1000.0,
                ..Default::default()
            })
            .collect();

        let config = IndicatorConfig {
            indicator_type: IndicatorType::SMA,
            params: crate::models::strategy::IndicatorParams {
                period: Some(10),
                ..Default::default()
            },
            output_field: None,
        };

        let output = compute_indicator(&config, &candles).unwrap();
        assert_eq!(output.primary.len(), 30);
        assert!(output.primary[9].is_finite());
        assert!(output.secondary.is_none());
    }
}
