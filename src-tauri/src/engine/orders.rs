use rand::Rng;

use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::strategy::{CommissionType, TradeDirection, TradingCosts};

// ── Bid/Ask split ──────────────────────────────────────────────────────────

/// Synthetic bid and ask OHLC derived from a mid-price candle.
///
/// In bar-mode the candle data is treated as bid prices (standard Forex convention).
/// Ask = bid + full_spread. SL/TP for long positions are checked against bid,
/// SL/TP for short positions are checked against ask.
#[derive(Debug, Clone, Copy)]
pub struct BidAskOhlc {
    pub bid_open: f64,
    pub bid_high: f64,
    pub bid_low: f64,
    pub bid_close: f64,
    pub ask_open: f64,
    pub ask_high: f64,
    pub ask_low: f64,
}

impl BidAskOhlc {
    /// Build from a mid/bid candle and the full spread in price units.
    pub fn from_candle(candle: &Candle, spread: f64) -> Self {
        BidAskOhlc {
            bid_open: candle.open,
            bid_high: candle.high,
            bid_low: candle.low,
            bid_close: candle.close,
            ask_open: candle.open + spread,
            ask_high: candle.high + spread,
            ask_low: candle.low + spread,
        }
    }
}

/// Compute the full spread in price units from costs + instrument config.
pub fn spread_price(costs: &TradingCosts, instrument: &InstrumentConfig) -> f64 {
    costs.spread_pips * instrument.pip_size
}

/// Apply trading costs (spread + slippage) to the entry price.
/// For long: buy at ask (price + spread), for short: sell at bid (price - spread).
pub fn apply_entry_costs<R: Rng>(
    price: f64,
    direction: TradeDirection,
    costs: &TradingCosts,
    instrument: &InstrumentConfig,
    rng: &mut R,
) -> f64 {
    let spread = costs.spread_pips * instrument.pip_size;
    let slippage = if costs.slippage_random {
        // Random slippage between 0 and max — uses caller-provided RNG for reproducibility
        costs.slippage_pips * instrument.pip_size * rng.gen::<f64>()
    } else {
        costs.slippage_pips * instrument.pip_size
    };

    match direction {
        TradeDirection::Long | TradeDirection::Both => price + spread + slippage,
        TradeDirection::Short => price - spread - slippage,
    }
}

/// Apply trading costs (slippage) to the exit price.
/// For long: sell at bid (price - slippage), for short: buy at ask (price + slippage).
/// Note: spread is already paid on entry, only slippage affects exit.
pub fn apply_exit_costs<R: Rng>(
    price: f64,
    direction: TradeDirection,
    costs: &TradingCosts,
    instrument: &InstrumentConfig,
    rng: &mut R,
) -> f64 {
    let slippage = if costs.slippage_random {
        costs.slippage_pips * instrument.pip_size * rng.gen::<f64>()
    } else {
        costs.slippage_pips * instrument.pip_size
    };

    match direction {
        // Long exit = selling → price moves against us (lower)
        TradeDirection::Long | TradeDirection::Both => price - slippage,
        // Short exit = buying → price moves against us (higher)
        TradeDirection::Short => price + slippage,
    }
}

/// Calculate monetary P&L for a closed position.
pub fn calculate_pnl(
    direction: TradeDirection,
    entry_price: f64,
    exit_price: f64,
    lots: f64,
    instrument: &InstrumentConfig,
) -> f64 {
    let pnl_pips = calculate_pnl_pips(direction, entry_price, exit_price, instrument);
    pnl_pips * instrument.pip_value * lots
}

/// Calculate P&L in pips.
pub fn calculate_pnl_pips(
    direction: TradeDirection,
    entry_price: f64,
    exit_price: f64,
    instrument: &InstrumentConfig,
) -> f64 {
    if instrument.pip_size == 0.0 {
        return 0.0;
    }
    match direction {
        TradeDirection::Long | TradeDirection::Both => {
            (exit_price - entry_price) / instrument.pip_size
        }
        TradeDirection::Short => (entry_price - exit_price) / instrument.pip_size,
    }
}

/// Calculate commission for a trade.
pub fn calculate_commission(
    costs: &TradingCosts,
    lots: f64,
    entry_price: f64,
    instrument: &InstrumentConfig,
) -> f64 {
    match costs.commission_type {
        CommissionType::FixedPerLot => costs.commission_value * lots,
        CommissionType::Percentage => {
            let position_value = entry_price * lots * instrument.lot_size;
            position_value * costs.commission_value / 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forex_instrument() -> InstrumentConfig {
        InstrumentConfig::default()
    }

    #[test]
    fn test_pnl_long() {
        let inst = forex_instrument();
        let pnl_pips = calculate_pnl_pips(TradeDirection::Long, 1.1000, 1.1050, &inst);
        assert!((pnl_pips - 50.0).abs() < 0.01);
        let pnl = calculate_pnl(TradeDirection::Long, 1.1000, 1.1050, 1.0, &inst);
        assert!((pnl - 500.0).abs() < 0.01); // 50 pips * $10/pip * 1 lot
    }

    #[test]
    fn test_pnl_short() {
        let inst = forex_instrument();
        let pnl_pips = calculate_pnl_pips(TradeDirection::Short, 1.1050, 1.1000, &inst);
        assert!((pnl_pips - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_apply_entry_costs_long() {
        let inst = forex_instrument();
        let costs = TradingCosts {
            spread_pips: 2.0,
            commission_type: CommissionType::FixedPerLot,
            commission_value: 7.0,
            slippage_pips: 0.0,
            slippage_random: false,
        };
        let adjusted = apply_entry_costs(1.1000, TradeDirection::Long, &costs, &inst, &mut rand::thread_rng());
        // Long: price + spread = 1.1000 + 2*0.0001 = 1.1002
        assert!((adjusted - 1.1002).abs() < 1e-10);
    }

    #[test]
    fn test_commission_fixed_per_lot() {
        let inst = forex_instrument();
        let costs = TradingCosts {
            spread_pips: 0.0,
            commission_type: CommissionType::FixedPerLot,
            commission_value: 7.0,
            slippage_pips: 0.0,
            slippage_random: false,
        };
        let comm = calculate_commission(&costs, 2.0, 1.1000, &inst);
        assert!((comm - 14.0).abs() < 1e-10); // $7 * 2 lots
    }
}
