use std::collections::HashMap;

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
    /// Extra named outputs for indicators with >3 outputs (e.g. Ichimoku, Pivots).
    pub extra: Option<HashMap<String, Vec<f64>>>,
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
    let open: Vec<f64> = candles.iter().map(|c| c.open).collect();

    match config.indicator_type {
        IndicatorType::SMA => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: sma(&close, period),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::EMA => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: ema(&close, period),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::RSI => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: rsi(&close, period),
                secondary: None,
                tertiary: None,
                extra: None,
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
                extra: None,
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
                extra: None,
            })
        }
        IndicatorType::ATR => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: atr(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
                extra: None,
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
                extra: None,
            })
        }
        IndicatorType::ADX => {
            let period = require_period(&config.params)?;
            check_data_len(len, period * 2 + 1)?;
            Ok(IndicatorOutput {
                primary: adx(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::CCI => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: cci(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::ROC => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput {
                primary: roc(&close, period),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::WilliamsR => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput {
                primary: williams_r(&high, &low, &close, period),
                secondary: None,
                tertiary: None,
                extra: None,
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
                extra: None,
            })
        }
        IndicatorType::VWAP => {
            check_data_len(len, 1)?;
            Ok(IndicatorOutput {
                primary: vwap(&high, &low, &close, &volume, candles),
                secondary: None,
                tertiary: None,
                extra: None,
            })
        }
        IndicatorType::Aroon => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            let (up, down) = aroon(&high, &low, period);
            Ok(IndicatorOutput { primary: up, secondary: Some(down), tertiary: None, extra: None })
        }
        IndicatorType::AwesomeOscillator => {
            check_data_len(len, 34)?;
            Ok(IndicatorOutput { primary: awesome_oscillator(&high, &low), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::BarRange => {
            Ok(IndicatorOutput { primary: bar_range(&high, &low), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::BiggestRange => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: biggest_range(&high, &low, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::HighestInRange => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: highest_in_range(&high, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::LowestInRange => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: lowest_in_range(&low, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::SmallestRange => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: smallest_range(&high, &low, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::BearsPower => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: bears_power(&low, &close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::BullsPower => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: bulls_power(&high, &close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::DeMarker => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput { primary: demarker(&high, &low, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Fibonacci => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            let extra = fibonacci(&high, &low, period);
            let primary = extra.get("level_500").cloned().unwrap_or_else(|| vec![f64::NAN; len]);
            Ok(IndicatorOutput { primary, secondary: None, tertiary: None, extra: Some(extra) })
        }
        IndicatorType::Fractal => {
            check_data_len(len, 5)?;
            let (up, down) = fractal(&high, &low);
            Ok(IndicatorOutput { primary: up, secondary: Some(down), tertiary: None, extra: None })
        }
        IndicatorType::GannHiLo => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: gann_hilo(&high, &low, &close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::HeikenAshi => {
            let (ha_close, ha_open) = heiken_ashi(&open, &high, &low, &close);
            Ok(IndicatorOutput { primary: ha_close, secondary: Some(ha_open), tertiary: None, extra: None })
        }
        IndicatorType::HullMA => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: hull_ma(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Ichimoku => {
            let fast = config.params.fast_period.unwrap_or(9);
            let slow = config.params.slow_period.unwrap_or(26);
            let senkou_b_period = config.params.signal_period.unwrap_or(52);
            check_data_len(len, senkou_b_period + slow)?;
            let extra = ichimoku(&high, &low, &close, fast, slow, senkou_b_period);
            let primary = extra.get("tenkan").cloned().unwrap_or_else(|| vec![f64::NAN; len]);
            Ok(IndicatorOutput { primary, secondary: None, tertiary: None, extra: Some(extra) })
        }
        IndicatorType::KeltnerChannel => {
            let period = require_period(&config.params)?;
            let mult = config.params.multiplier.unwrap_or(1.5);
            check_data_len(len, period + 1)?;
            let (upper, middle, lower) = keltner_channel(&high, &low, &close, period, mult);
            Ok(IndicatorOutput { primary: middle, secondary: Some(upper), tertiary: Some(lower), extra: None })
        }
        IndicatorType::LaguerreRSI => {
            let gamma = config.params.gamma.unwrap_or(0.8);
            Ok(IndicatorOutput { primary: laguerre_rsi(&close, gamma), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::LinearRegression => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: linear_regression(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Momentum => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput { primary: momentum(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::SuperTrend => {
            let period = require_period(&config.params)?;
            let mult = config.params.multiplier.unwrap_or(3.0);
            check_data_len(len, period + 1)?;
            Ok(IndicatorOutput { primary: supertrend(&high, &low, &close, period, mult), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::TrueRange => {
            Ok(IndicatorOutput { primary: true_range(&high, &low, &close), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::StdDev => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: std_dev(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Reflex => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 2)?;
            Ok(IndicatorOutput { primary: reflex(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Pivots => {
            let extra = pivots(candles);
            let primary = extra.get("pp").cloned().unwrap_or_else(|| vec![f64::NAN; len]);
            Ok(IndicatorOutput { primary, secondary: None, tertiary: None, extra: Some(extra) })
        }
        IndicatorType::UlcerIndex => {
            let period = require_period(&config.params)?;
            check_data_len(len, period)?;
            Ok(IndicatorOutput { primary: ulcer_index(&close, period), secondary: None, tertiary: None, extra: None })
        }
        IndicatorType::Vortex => {
            let period = require_period(&config.params)?;
            check_data_len(len, period + 1)?;
            let (vi_plus, vi_minus) = vortex(&high, &low, &close, period);
            Ok(IndicatorOutput { primary: vi_plus, secondary: Some(vi_minus), tertiary: None, extra: None })
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

// ── WMA (helper for Hull MA) ──

/// Weighted Moving Average. First `period-1` values are NaN.
fn wma(data: &[f64], period: usize) -> Vec<f64> {
    let len = data.len();
    let mut result = vec![f64::NAN; len];
    if period == 0 || len < period {
        return result;
    }
    let denom = (period * (period + 1)) as f64 / 2.0;
    for i in (period - 1)..len {
        let mut sum = 0.0;
        let mut all_valid = true;
        for j in 0..period {
            let idx = i + 1 - period + j;
            if data[idx].is_nan() {
                all_valid = false;
                break;
            }
            sum += data[idx] * (j + 1) as f64;
        }
        if all_valid {
            result[i] = sum / denom;
        }
    }
    result
}

// ── Aroon ──

/// Aroon Up/Down oscillator. Returns (aroon_up, aroon_down).
fn aroon(high: &[f64], low: &[f64], period: usize) -> (Vec<f64>, Vec<f64>) {
    let len = high.len();
    let mut up = vec![f64::NAN; len];
    let mut down = vec![f64::NAN; len];
    for i in period..len {
        let start = i - period;
        let mut max_idx = start;
        let mut min_idx = start;
        for j in start..=i {
            if high[j] >= high[max_idx] {
                max_idx = j;
            }
            if low[j] <= low[min_idx] {
                min_idx = j;
            }
        }
        up[i] = ((period as f64 - (i - max_idx) as f64) / period as f64) * 100.0;
        down[i] = ((period as f64 - (i - min_idx) as f64) / period as f64) * 100.0;
    }
    (up, down)
}

// ── Awesome Oscillator ──

/// Awesome Oscillator = SMA(midpoint, 5) - SMA(midpoint, 34).
fn awesome_oscillator(high: &[f64], low: &[f64]) -> Vec<f64> {
    let len = high.len();
    let midpoint: Vec<f64> = (0..len).map(|i| (high[i] + low[i]) / 2.0).collect();
    let sma5 = sma(&midpoint, 5);
    let sma34 = sma(&midpoint, 34);
    let mut result = vec![f64::NAN; len];
    for i in 0..len {
        if !sma5[i].is_nan() && !sma34[i].is_nan() {
            result[i] = sma5[i] - sma34[i];
        }
    }
    result
}

// ── BarRange ──

/// Bar Range = High - Low for each bar.
fn bar_range(high: &[f64], low: &[f64]) -> Vec<f64> {
    high.iter().zip(low.iter()).map(|(h, l)| h - l).collect()
}

// ── BiggestRange ──

/// Biggest bar range (H-L) over a rolling window of `period` bars.
fn biggest_range(high: &[f64], low: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let mut max_range = f64::NEG_INFINITY;
        for j in (i + 1 - period)..=i {
            max_range = max_range.max(high[j] - low[j]);
        }
        result[i] = max_range;
    }
    result
}

// ── HighestInRange ──

/// Highest high over a rolling window of `period` bars.
fn highest_in_range(high: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let mut max_val = f64::NEG_INFINITY;
        for j in (i + 1 - period)..=i {
            max_val = max_val.max(high[j]);
        }
        result[i] = max_val;
    }
    result
}

// ── LowestInRange ──

/// Lowest low over a rolling window of `period` bars.
fn lowest_in_range(low: &[f64], period: usize) -> Vec<f64> {
    let len = low.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let mut min_val = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            min_val = min_val.min(low[j]);
        }
        result[i] = min_val;
    }
    result
}

// ── SmallestRange ──

/// Smallest bar range (H-L) over a rolling window of `period` bars.
fn smallest_range(high: &[f64], low: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let mut min_range = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            min_range = min_range.min(high[j] - low[j]);
        }
        result[i] = min_range;
    }
    result
}

// ── Bears Power ──

/// Bears Power = Low - EMA(Close, period).
fn bears_power(low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let ema_vals = ema(close, period);
    low.iter()
        .zip(ema_vals.iter())
        .map(|(l, e)| if e.is_nan() { f64::NAN } else { l - e })
        .collect()
}

// ── Bulls Power ──

/// Bulls Power = High - EMA(Close, period).
fn bulls_power(high: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let ema_vals = ema(close, period);
    high.iter()
        .zip(ema_vals.iter())
        .map(|(h, e)| if e.is_nan() { f64::NAN } else { h - e })
        .collect()
}

// ── DeMarker ──

/// DeMarker oscillator (0..1 range).
fn demarker(high: &[f64], low: &[f64], period: usize) -> Vec<f64> {
    let len = high.len();
    let mut de_max = vec![f64::NAN; len];
    let mut de_min = vec![f64::NAN; len];

    for i in 1..len {
        de_max[i] = (high[i] - high[i - 1]).max(0.0);
        de_min[i] = (low[i - 1] - low[i]).max(0.0);
    }

    let sma_max = sma_on_slice(&de_max, period);
    let sma_min = sma_on_slice(&de_min, period);

    let mut result = vec![f64::NAN; len];
    for i in 0..len {
        if !sma_max[i].is_nan() && !sma_min[i].is_nan() {
            let total = sma_max[i] + sma_min[i];
            result[i] = if total == 0.0 { 0.5 } else { sma_max[i] / total };
        }
    }
    result
}

// ── Fibonacci Retracement ──

/// Rolling Fibonacci retracement levels from HH/LL over `period` bars.
/// Returns extra map with keys: level_236, level_382, level_500, level_618, level_786.
fn fibonacci(high: &[f64], low: &[f64], period: usize) -> HashMap<String, Vec<f64>> {
    let len = high.len();
    let mut level_236 = vec![f64::NAN; len];
    let mut level_382 = vec![f64::NAN; len];
    let mut level_500 = vec![f64::NAN; len];
    let mut level_618 = vec![f64::NAN; len];
    let mut level_786 = vec![f64::NAN; len];

    for i in (period - 1)..len {
        let mut hh = f64::NEG_INFINITY;
        let mut ll = f64::INFINITY;
        for j in (i + 1 - period)..=i {
            hh = hh.max(high[j]);
            ll = ll.min(low[j]);
        }
        let range = hh - ll;
        level_236[i] = hh - range * 0.236;
        level_382[i] = hh - range * 0.382;
        level_500[i] = hh - range * 0.500;
        level_618[i] = hh - range * 0.618;
        level_786[i] = hh - range * 0.786;
    }

    let mut map = HashMap::new();
    map.insert("level_236".to_string(), level_236);
    map.insert("level_382".to_string(), level_382);
    map.insert("level_500".to_string(), level_500);
    map.insert("level_618".to_string(), level_618);
    map.insert("level_786".to_string(), level_786);
    map
}

// ── Fractal ──

/// Williams 5-bar fractal. Returns (fractal_up, fractal_down).
/// Values are the price level of the fractal, or NaN if no fractal.
/// Confirmed 2 bars after the peak/trough.
fn fractal(high: &[f64], low: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let len = high.len();
    let mut up = vec![f64::NAN; len];
    let mut down = vec![f64::NAN; len];

    for i in 4..len {
        let mid = i - 2;
        if high[mid] > high[mid - 2]
            && high[mid] > high[mid - 1]
            && high[mid] > high[mid + 1]
            && high[mid] > high[mid + 2]
        {
            up[i] = high[mid];
        }
        if low[mid] < low[mid - 2]
            && low[mid] < low[mid - 1]
            && low[mid] < low[mid + 1]
            && low[mid] < low[mid + 2]
        {
            down[i] = low[mid];
        }
    }

    (up, down)
}

// ── Gann HiLo Activator ──

/// Gann HiLo Activator. Outputs SMA(low) when bullish, SMA(high) when bearish.
fn gann_hilo(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let sma_h = sma(high, period);
    let sma_l = sma(low, period);
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    let mut is_bullish = true;

    for i in (period - 1)..len {
        if sma_h[i].is_nan() || sma_l[i].is_nan() {
            continue;
        }
        if close[i] > sma_h[i] {
            is_bullish = true;
        } else if close[i] < sma_l[i] {
            is_bullish = false;
        }
        result[i] = if is_bullish { sma_l[i] } else { sma_h[i] };
    }
    result
}

// ── Heiken Ashi ──

/// Heiken Ashi candles. Returns (ha_close, ha_open).
fn heiken_ashi(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
) -> (Vec<f64>, Vec<f64>) {
    let len = open.len();
    let mut ha_close = vec![f64::NAN; len];
    let mut ha_open = vec![f64::NAN; len];
    if len == 0 {
        return (ha_close, ha_open);
    }

    ha_close[0] = (open[0] + high[0] + low[0] + close[0]) / 4.0;
    ha_open[0] = (open[0] + close[0]) / 2.0;

    for i in 1..len {
        ha_close[i] = (open[i] + high[i] + low[i] + close[i]) / 4.0;
        ha_open[i] = (ha_open[i - 1] + ha_close[i - 1]) / 2.0;
    }

    (ha_close, ha_open)
}

// ── Hull Moving Average ──

/// Hull MA = WMA(2*WMA(n/2) - WMA(n), sqrt(n)).
fn hull_ma(close: &[f64], period: usize) -> Vec<f64> {
    let half = (period / 2).max(1);
    let sqrt_p = ((period as f64).sqrt() as usize).max(1);

    let wma_half = wma(close, half);
    let wma_full = wma(close, period);

    let len = close.len();
    let mut diff = vec![f64::NAN; len];
    for i in 0..len {
        if !wma_half[i].is_nan() && !wma_full[i].is_nan() {
            diff[i] = 2.0 * wma_half[i] - wma_full[i];
        }
    }

    wma(&diff, sqrt_p)
}

// ── Ichimoku ──

/// Ichimoku Kinko Hyo. Returns extra map with keys: tenkan, kijun, senkou_a, senkou_b, chikou.
fn ichimoku(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    tenkan_period: usize,
    kijun_period: usize,
    senkou_b_period: usize,
) -> HashMap<String, Vec<f64>> {
    let len = high.len();

    // Helper: midpoint of highest high and lowest low over a period
    let midpoint = |period: usize| -> Vec<f64> {
        let mut result = vec![f64::NAN; len];
        if period == 0 {
            return result;
        }
        for i in (period - 1)..len {
            let mut hh = f64::NEG_INFINITY;
            let mut ll = f64::INFINITY;
            for j in (i + 1 - period)..=i {
                hh = hh.max(high[j]);
                ll = ll.min(low[j]);
            }
            result[i] = (hh + ll) / 2.0;
        }
        result
    };

    let tenkan = midpoint(tenkan_period);
    let kijun = midpoint(kijun_period);

    // Senkou Span A: projected forward kijun_period bars
    // senkou_a[i] = (tenkan[i - kijun_period] + kijun[i - kijun_period]) / 2
    let mut senkou_a = vec![f64::NAN; len];
    for i in kijun_period..len {
        let src = i - kijun_period;
        if !tenkan[src].is_nan() && !kijun[src].is_nan() {
            senkou_a[i] = (tenkan[src] + kijun[src]) / 2.0;
        }
    }

    // Senkou Span B: projected forward kijun_period bars
    let senkou_b_raw = midpoint(senkou_b_period);
    let mut senkou_b = vec![f64::NAN; len];
    for i in kijun_period..len {
        let src = i - kijun_period;
        if !senkou_b_raw[src].is_nan() {
            senkou_b[i] = senkou_b_raw[src];
        }
    }

    // Chikou = close (no look-ahead for backtesting)
    let chikou = close.to_vec();

    let mut map = HashMap::new();
    map.insert("tenkan".to_string(), tenkan);
    map.insert("kijun".to_string(), kijun);
    map.insert("senkou_a".to_string(), senkou_a);
    map.insert("senkou_b".to_string(), senkou_b);
    map.insert("chikou".to_string(), chikou);
    map
}

// ── Keltner Channel ──

/// Keltner Channel: returns (upper, middle, lower).
fn keltner_channel(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    period: usize,
    multiplier: f64,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let middle = ema(close, period);
    let atr_vals = atr(high, low, close, period);
    let len = close.len();
    let mut upper = vec![f64::NAN; len];
    let mut lower = vec![f64::NAN; len];

    for i in 0..len {
        if !middle[i].is_nan() && !atr_vals[i].is_nan() {
            upper[i] = middle[i] + multiplier * atr_vals[i];
            lower[i] = middle[i] - multiplier * atr_vals[i];
        }
    }

    (upper, middle, lower)
}

// ── Laguerre RSI ──

/// Laguerre RSI (0..1 range). Uses gamma smoothing parameter.
fn laguerre_rsi(close: &[f64], gamma: f64) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    if len == 0 {
        return result;
    }

    let mut l0 = 0.0f64;
    let mut l1 = 0.0f64;
    let mut l2 = 0.0f64;
    let mut l3 = 0.0f64;

    for i in 0..len {
        let prev_l0 = l0;
        let prev_l1 = l1;
        let prev_l2 = l2;

        l0 = (1.0 - gamma) * close[i] + gamma * prev_l0;
        l1 = -gamma * l0 + prev_l0 + gamma * prev_l1;
        l2 = -gamma * l1 + prev_l1 + gamma * prev_l2;
        l3 = -gamma * l2 + prev_l2 + gamma * l3;

        let mut cu = 0.0;
        let mut cd = 0.0;

        if l0 >= l1 { cu += l0 - l1; } else { cd += l1 - l0; }
        if l1 >= l2 { cu += l1 - l2; } else { cd += l2 - l1; }
        if l2 >= l3 { cu += l2 - l3; } else { cd += l3 - l2; }

        let total = cu + cd;
        result[i] = if total != 0.0 { cu / total } else { 0.0 };
    }
    result
}

// ── Linear Regression ──

/// Linear Regression fitted value at last bar of rolling window.
fn linear_regression(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];

    for i in (period - 1)..len {
        let window = &close[i + 1 - period..=i];
        let n = period as f64;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;

        for (j, &y) in window.iter().enumerate() {
            let x = j as f64;
            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let denom = n * sum_x2 - sum_x * sum_x;
        if denom != 0.0 {
            let b = (n * sum_xy - sum_x * sum_y) / denom;
            let a = (sum_y - b * sum_x) / n;
            result[i] = a + b * (n - 1.0);
        }
    }
    result
}

// ── Momentum ──

/// Momentum = Close - Close[period].
fn momentum(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    for i in period..len {
        result[i] = close[i] - close[i - period];
    }
    result
}

// ── SuperTrend ──

/// SuperTrend indicator. Outputs the SuperTrend line (lower band when bullish, upper when bearish).
fn supertrend(
    high: &[f64],
    low: &[f64],
    close: &[f64],
    period: usize,
    multiplier: f64,
) -> Vec<f64> {
    let len = high.len();
    let atr_vals = atr(high, low, close, period);
    let mut result = vec![f64::NAN; len];
    let mut final_upper = vec![f64::NAN; len];
    let mut final_lower = vec![f64::NAN; len];
    let mut supertrend_is_upper = false;

    let first_valid = period - 1;
    if first_valid >= len {
        return result;
    }

    for i in first_valid..len {
        if atr_vals[i].is_nan() {
            continue;
        }

        let hl2 = (high[i] + low[i]) / 2.0;
        let basic_upper = hl2 + multiplier * atr_vals[i];
        let basic_lower = hl2 - multiplier * atr_vals[i];

        if i == first_valid {
            final_upper[i] = basic_upper;
            final_lower[i] = basic_lower;
            supertrend_is_upper = close[i] <= basic_upper;
            result[i] = if supertrend_is_upper {
                final_upper[i]
            } else {
                final_lower[i]
            };
            continue;
        }

        final_upper[i] =
            if basic_upper < final_upper[i - 1] || close[i - 1] > final_upper[i - 1] {
                basic_upper
            } else {
                final_upper[i - 1]
            };

        final_lower[i] =
            if basic_lower > final_lower[i - 1] || close[i - 1] < final_lower[i - 1] {
                basic_lower
            } else {
                final_lower[i - 1]
            };

        if supertrend_is_upper {
            if close[i] > final_upper[i] {
                supertrend_is_upper = false;
            }
        } else if close[i] < final_lower[i] {
            supertrend_is_upper = true;
        }

        result[i] = if supertrend_is_upper {
            final_upper[i]
        } else {
            final_lower[i]
        };
    }
    result
}

// ── True Range ──

/// True Range = max(H-L, |H-prevC|, |L-prevC|). First bar = H-L.
fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let len = high.len();
    let mut result = vec![f64::NAN; len];
    if len == 0 {
        return result;
    }
    result[0] = high[0] - low[0];
    for i in 1..len {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        result[i] = hl.max(hc).max(lc);
    }
    result
}

// ── Standard Deviation ──

/// Rolling standard deviation of close over `period` bars.
fn std_dev(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    for i in (period - 1)..len {
        let window = &close[i + 1 - period..=i];
        let mean = window.iter().sum::<f64>() / period as f64;
        let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
        result[i] = variance.sqrt();
    }
    result
}

// ── Reflex ──

/// Ehlers Reflex indicator. Super Smoother + cycle measurement.
fn reflex(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];
    if len < 3 {
        return result;
    }

    let pi = std::f64::consts::PI;
    let sqrt2 = std::f64::consts::SQRT_2;
    let a1 = (-sqrt2 * pi / period as f64).exp();
    let coeff2 = 2.0 * a1 * (sqrt2 * pi / period as f64).cos();
    let coeff3 = -(a1 * a1);
    let coeff1 = 1.0 - coeff2 - coeff3;

    // Super Smoother filter
    let mut filt = vec![0.0f64; len];
    filt[0] = close[0];
    filt[1] = close[1];
    for i in 2..len {
        filt[i] = coeff1 * (close[i] + close[i - 1]) / 2.0 + coeff2 * filt[i - 1]
            + coeff3 * filt[i - 2];
    }

    // Reflex computation
    let mut ms = 0.0f64;
    for i in period..len {
        let slope = (filt[i - period] - filt[i]) / period as f64;
        let mut sum = 0.0;
        for j in 1..=period {
            sum += (filt[i] + j as f64 * slope) - filt[i - j];
        }
        sum /= period as f64;
        ms = 0.04 * sum * sum + 0.96 * ms;
        result[i] = if ms > 0.0 { sum / ms.sqrt() } else { 0.0 };
    }
    result
}

// ── Pivots ──

/// Classic pivot points from previous day's HLC.
/// Returns extra map: pp, r1, r2, r3, s1, s2, s3.
fn pivots(candles: &[Candle]) -> HashMap<String, Vec<f64>> {
    let len = candles.len();
    let mut pp = vec![f64::NAN; len];
    let mut r1 = vec![f64::NAN; len];
    let mut r2 = vec![f64::NAN; len];
    let mut r3 = vec![f64::NAN; len];
    let mut s1 = vec![f64::NAN; len];
    let mut s2 = vec![f64::NAN; len];
    let mut s3 = vec![f64::NAN; len];

    let mut prev_day_high = f64::NAN;
    let mut prev_day_low = f64::NAN;
    let mut prev_day_close = f64::NAN;

    let mut current_day_high = f64::NEG_INFINITY;
    let mut current_day_low = f64::INFINITY;
    let mut current_day_close = 0.0f64;
    let mut prev_date = String::new();
    let mut day_started = false;

    for i in 0..len {
        let current_date = if candles[i].datetime.len() >= 10 {
            &candles[i].datetime[..10]
        } else {
            &candles[i].datetime
        };

        if current_date != prev_date {
            if day_started {
                prev_day_high = current_day_high;
                prev_day_low = current_day_low;
                prev_day_close = current_day_close;
            }
            current_day_high = candles[i].high;
            current_day_low = candles[i].low;
            current_day_close = candles[i].close;
            prev_date = current_date.to_string();
            day_started = true;
        } else {
            current_day_high = current_day_high.max(candles[i].high);
            current_day_low = current_day_low.min(candles[i].low);
            current_day_close = candles[i].close;
        }

        if !prev_day_high.is_nan() {
            let pivot = (prev_day_high + prev_day_low + prev_day_close) / 3.0;
            pp[i] = pivot;
            r1[i] = 2.0 * pivot - prev_day_low;
            s1[i] = 2.0 * pivot - prev_day_high;
            r2[i] = pivot + (prev_day_high - prev_day_low);
            s2[i] = pivot - (prev_day_high - prev_day_low);
            r3[i] = prev_day_high + 2.0 * (pivot - prev_day_low);
            s3[i] = prev_day_low - 2.0 * (prev_day_high - pivot);
        }
    }

    let mut map = HashMap::new();
    map.insert("pp".to_string(), pp);
    map.insert("r1".to_string(), r1);
    map.insert("r2".to_string(), r2);
    map.insert("r3".to_string(), r3);
    map.insert("s1".to_string(), s1);
    map.insert("s2".to_string(), s2);
    map.insert("s3".to_string(), s3);
    map
}

// ── Ulcer Index ──

/// Ulcer Index = RMS of percentage drawdown from rolling max.
fn ulcer_index(close: &[f64], period: usize) -> Vec<f64> {
    let len = close.len();
    let mut result = vec![f64::NAN; len];

    for i in (period - 1)..len {
        let window = &close[i + 1 - period..=i];
        let mut max_close = f64::NEG_INFINITY;
        let mut sum_sq = 0.0;
        for &val in window {
            max_close = max_close.max(val);
            let pct_dd = (val - max_close) / max_close * 100.0;
            sum_sq += pct_dd * pct_dd;
        }
        result[i] = (sum_sq / period as f64).sqrt();
    }
    result
}

// ── Vortex ──

/// Vortex indicator. Returns (VI+, VI-).
fn vortex(high: &[f64], low: &[f64], close: &[f64], period: usize) -> (Vec<f64>, Vec<f64>) {
    let len = high.len();
    let mut vi_plus = vec![f64::NAN; len];
    let mut vi_minus = vec![f64::NAN; len];

    let mut vm_plus = vec![0.0f64; len];
    let mut vm_minus = vec![0.0f64; len];
    let mut tr = vec![0.0f64; len];

    tr[0] = high[0] - low[0];
    for i in 1..len {
        vm_plus[i] = (high[i] - low[i - 1]).abs();
        vm_minus[i] = (low[i] - high[i - 1]).abs();
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    for i in period..len {
        let sum_vm_plus: f64 = vm_plus[(i + 1 - period)..=i].iter().sum();
        let sum_vm_minus: f64 = vm_minus[(i + 1 - period)..=i].iter().sum();
        let sum_tr: f64 = tr[(i + 1 - period)..=i].iter().sum();
        if sum_tr != 0.0 {
            vi_plus[i] = sum_vm_plus / sum_tr;
            vi_minus[i] = sum_vm_minus / sum_tr;
        }
    }

    (vi_plus, vi_minus)
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
