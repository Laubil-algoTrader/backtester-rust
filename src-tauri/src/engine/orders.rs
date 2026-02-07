use crate::models::config::InstrumentConfig;
use crate::models::strategy::{CommissionType, TradeDirection, TradingCosts};

/// Apply trading costs (spread + slippage) to the entry price.
/// For long: buy at ask (price + spread), for short: sell at bid (price - spread).
pub fn apply_entry_costs(
    price: f64,
    direction: TradeDirection,
    costs: &TradingCosts,
    instrument: &InstrumentConfig,
) -> f64 {
    let spread = costs.spread_pips * instrument.pip_size;
    let slippage = if costs.slippage_random {
        // Random slippage between 0 and max
        let random_factor = rand::random::<f64>();
        costs.slippage_pips * instrument.pip_size * random_factor
    } else {
        costs.slippage_pips * instrument.pip_size
    };

    match direction {
        TradeDirection::Long | TradeDirection::Both => price + spread + slippage,
        TradeDirection::Short => price - spread - slippage,
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
        InstrumentConfig {
            pip_size: 0.0001,
            pip_value: 10.0,
            lot_size: 100_000.0,
            min_lot: 0.01,
            tick_size: 0.00001,
            digits: 5,
        }
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
        let adjusted = apply_entry_costs(1.1000, TradeDirection::Long, &costs, &inst);
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
