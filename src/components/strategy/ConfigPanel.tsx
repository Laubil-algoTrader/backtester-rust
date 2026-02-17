import type {
  PositionSizing,
  PositionSizingType,
  StopLoss,
  StopLossType,
  TakeProfit,
  TakeProfitType,
  TrailingStop,
  TrailingStopType,
  TradingCosts,
  CommissionType,
  TradeDirection,
  TradingHours,
  CloseTradesAt,
} from "@/lib/types";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/Tabs";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";

interface ConfigPanelProps {
  positionSizing: PositionSizing;
  stopLoss?: StopLoss;
  takeProfit?: TakeProfit;
  trailingStop?: TrailingStop;
  tradingCosts: TradingCosts;
  tradeDirection: TradeDirection;
  tradingHours?: TradingHours;
  maxDailyTrades?: number;
  closeTradesAt?: CloseTradesAt;
  onPositionSizingChange: (ps: PositionSizing) => void;
  onStopLossChange: (sl: StopLoss | undefined) => void;
  onTakeProfitChange: (tp: TakeProfit | undefined) => void;
  onTrailingStopChange: (ts: TrailingStop | undefined) => void;
  onTradingCostsChange: (costs: TradingCosts) => void;
  onTradeDirectionChange: (dir: TradeDirection) => void;
  onTradingHoursChange: (hours: TradingHours | undefined) => void;
  onMaxDailyTradesChange: (max: number | undefined) => void;
  onCloseTradesAtChange: (ct: CloseTradesAt | undefined) => void;
}

const SIZING_TYPE_OPTIONS: { value: PositionSizingType; label: string }[] = [
  { value: "FixedLots", label: "Fixed Lots" },
  { value: "FixedAmount", label: "Fixed Amount" },
  { value: "PercentEquity", label: "% Equity" },
  { value: "RiskBased", label: "Risk Based" },
];

const SIZING_VALUE_LABELS: Record<PositionSizingType, string> = {
  FixedLots: "Lots",
  FixedAmount: "Amount ($)",
  PercentEquity: "Equity (%)",
  RiskBased: "Risk (%)",
};

const SL_TYPE_OPTIONS: { value: StopLossType; label: string }[] = [
  { value: "Pips", label: "Pips" },
  { value: "Percentage", label: "Percentage" },
  { value: "ATR", label: "ATR Multiplier" },
];

const TP_TYPE_OPTIONS: { value: TakeProfitType; label: string }[] = [
  { value: "Pips", label: "Pips" },
  { value: "RiskReward", label: "Risk:Reward" },
  { value: "ATR", label: "ATR Multiplier" },
];

const TS_TYPE_OPTIONS: { value: TrailingStopType; label: string }[] = [
  { value: "ATR", label: "ATR Multiplier" },
  { value: "RiskReward", label: "Risk:Reward" },
];

const COMMISSION_TYPE_OPTIONS: { value: CommissionType; label: string }[] = [
  { value: "FixedPerLot", label: "Fixed/Lot" },
  { value: "Percentage", label: "Percentage" },
];

function LabeledInput({
  label,
  value,
  onChange,
  min,
  step,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min?: number;
  step?: string;
}) {
  const floor = min ?? 0;
  return (
    <div className="space-y-1">
      <label className="text-xs text-muted-foreground">{label}</label>
      <Input
        type="number"
        className="h-8 text-xs"
        min={floor}
        step={step ?? "any"}
        value={value}
        onChange={(e) => {
          const v = Number(e.target.value);
          onChange(Number.isNaN(v) ? floor : Math.max(floor, v));
        }}
      />
    </div>
  );
}

function ToggleCheckbox({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-2 text-xs">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="h-4 w-4 rounded border-input"
      />
      <span>{label}</span>
    </label>
  );
}

export function ConfigPanel({
  positionSizing,
  stopLoss,
  takeProfit,
  trailingStop,
  tradingCosts,
  tradeDirection,
  tradingHours,
  maxDailyTrades,
  closeTradesAt,
  onPositionSizingChange,
  onStopLossChange,
  onTakeProfitChange,
  onTrailingStopChange,
  onTradingCostsChange,
  onTradeDirectionChange,
  onTradingHoursChange,
  onMaxDailyTradesChange,
  onCloseTradesAtChange,
}: ConfigPanelProps) {
  return (
    <Tabs defaultValue="sizing" className="w-full">
      <TabsList className="grid w-full grid-cols-3 lg:grid-cols-6">
        <TabsTrigger value="sizing" className="text-xs">Size</TabsTrigger>
        <TabsTrigger value="sl" className="text-xs">SL</TabsTrigger>
        <TabsTrigger value="tp" className="text-xs">TP</TabsTrigger>
        <TabsTrigger value="trail" className="text-xs">Trail</TabsTrigger>
        <TabsTrigger value="costs" className="text-xs">Costs</TabsTrigger>
        <TabsTrigger value="filters" className="text-xs">Filters</TabsTrigger>
      </TabsList>

      {/* Position Sizing */}
      <TabsContent value="sizing" className="space-y-3 pt-2">
        <Select
          value={positionSizing.sizing_type}
          onValueChange={(v) =>
            onPositionSizingChange({
              sizing_type: v as PositionSizingType,
              value: positionSizing.value,
            })
          }
        >
          <SelectTrigger className="h-8 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {SIZING_TYPE_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <LabeledInput
          label={SIZING_VALUE_LABELS[positionSizing.sizing_type]}
          value={positionSizing.value}
          onChange={(value) =>
            onPositionSizingChange({ ...positionSizing, value })
          }
        />
      </TabsContent>

      {/* Stop Loss */}
      <TabsContent value="sl" className="space-y-3 pt-2">
        <ToggleCheckbox
          label="Enable Stop Loss"
          checked={!!stopLoss}
          onChange={(checked) =>
            onStopLossChange(
              checked ? { sl_type: "Pips", value: 20 } : undefined
            )
          }
        />
        {stopLoss && (
          <>
            <Select
              value={stopLoss.sl_type}
              onValueChange={(v) => {
                const newSl: StopLoss = {
                  sl_type: v as StopLossType,
                  value: stopLoss.value,
                };
                if (v === "ATR") newSl.atr_period = stopLoss.atr_period ?? 14;
                onStopLossChange(newSl);
              }}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SL_TYPE_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {stopLoss.sl_type === "ATR" && (
              <LabeledInput
                label="ATR Period"
                value={stopLoss.atr_period ?? 14}
                onChange={(atr_period) =>
                  onStopLossChange({ ...stopLoss, atr_period })
                }
                min={1}
                step="1"
              />
            )}
            <LabeledInput
              label={stopLoss.sl_type === "ATR" ? "Multiplier" : "Value"}
              value={stopLoss.value}
              onChange={(value) =>
                onStopLossChange({ ...stopLoss, value })
              }
            />
          </>
        )}
      </TabsContent>

      {/* Take Profit */}
      <TabsContent value="tp" className="space-y-3 pt-2">
        <ToggleCheckbox
          label="Enable Take Profit"
          checked={!!takeProfit}
          onChange={(checked) =>
            onTakeProfitChange(
              checked ? { tp_type: "RiskReward", value: 2.0 } : undefined
            )
          }
        />
        {takeProfit && (
          <>
            <Select
              value={takeProfit.tp_type}
              onValueChange={(v) => {
                const newTp: TakeProfit = {
                  tp_type: v as TakeProfitType,
                  value: takeProfit.value,
                };
                if (v === "ATR") newTp.atr_period = takeProfit.atr_period ?? 14;
                onTakeProfitChange(newTp);
              }}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TP_TYPE_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {takeProfit.tp_type === "ATR" && (
              <LabeledInput
                label="ATR Period"
                value={takeProfit.atr_period ?? 14}
                onChange={(atr_period) =>
                  onTakeProfitChange({ ...takeProfit, atr_period })
                }
                min={1}
                step="1"
              />
            )}
            <LabeledInput
              label={takeProfit.tp_type === "ATR" ? "Multiplier" : "Value"}
              value={takeProfit.value}
              onChange={(value) =>
                onTakeProfitChange({ ...takeProfit, value })
              }
            />
          </>
        )}
      </TabsContent>

      {/* Trailing Stop */}
      <TabsContent value="trail" className="space-y-3 pt-2">
        <ToggleCheckbox
          label="Enable Trailing Stop"
          checked={!!trailingStop}
          onChange={(checked) =>
            onTrailingStopChange(
              checked ? { ts_type: "ATR", value: 2.0, atr_period: 14 } : undefined
            )
          }
        />
        {trailingStop && (
          <>
            <Select
              value={trailingStop.ts_type}
              onValueChange={(v) => {
                const newTs: TrailingStop = {
                  ts_type: v as TrailingStopType,
                  value: trailingStop.value,
                };
                if (v === "ATR") newTs.atr_period = trailingStop.atr_period ?? 14;
                onTrailingStopChange(newTs);
              }}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TS_TYPE_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {trailingStop.ts_type === "ATR" && (
              <LabeledInput
                label="ATR Period"
                value={trailingStop.atr_period ?? 14}
                onChange={(atr_period) =>
                  onTrailingStopChange({ ...trailingStop, atr_period })
                }
                min={1}
                step="1"
              />
            )}
            <LabeledInput
              label={trailingStop.ts_type === "ATR" ? "Multiplier" : "Value"}
              value={trailingStop.value}
              onChange={(value) =>
                onTrailingStopChange({ ...trailingStop, value })
              }
            />
          </>
        )}
      </TabsContent>

      {/* Costs */}
      <TabsContent value="costs" className="space-y-3 pt-2">
        <LabeledInput
          label="Spread (pips)"
          value={tradingCosts.spread_pips}
          onChange={(spread_pips) =>
            onTradingCostsChange({ ...tradingCosts, spread_pips })
          }
        />
        <div className="space-y-1">
          <label className="text-xs text-muted-foreground">Commission Type</label>
          <Select
            value={tradingCosts.commission_type}
            onValueChange={(v) =>
              onTradingCostsChange({
                ...tradingCosts,
                commission_type: v as CommissionType,
              })
            }
          >
            <SelectTrigger className="h-8 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {COMMISSION_TYPE_OPTIONS.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <LabeledInput
          label="Commission"
          value={tradingCosts.commission_value}
          onChange={(commission_value) =>
            onTradingCostsChange({ ...tradingCosts, commission_value })
          }
        />
        <LabeledInput
          label="Slippage (pips)"
          value={tradingCosts.slippage_pips}
          onChange={(slippage_pips) =>
            onTradingCostsChange({ ...tradingCosts, slippage_pips })
          }
        />
        <ToggleCheckbox
          label="Random slippage"
          checked={tradingCosts.slippage_random}
          onChange={(slippage_random) =>
            onTradingCostsChange({ ...tradingCosts, slippage_random })
          }
        />
      </TabsContent>

      {/* Filters: Direction + Trading Hours + Daily Limit */}
      <TabsContent value="filters" className="space-y-4 pt-2">
        {/* Direction */}
        <div className="space-y-1">
          <label className="text-xs text-muted-foreground">Direction</label>
          <div className="flex gap-1">
            {(["Long", "Short", "Both"] as TradeDirection[]).map((dir) => (
              <Button
                key={dir}
                variant={tradeDirection === dir ? "default" : "outline"}
                size="sm"
                className="flex-1 text-xs"
                onClick={() => onTradeDirectionChange(dir)}
              >
                {dir === "Both" ? "Both" : `${dir} Only`}
              </Button>
            ))}
          </div>
        </div>

        {/* Trading Hours */}
        <div className="space-y-2">
          <ToggleCheckbox
            label="Limit Trading Hours"
            checked={!!tradingHours}
            onChange={(checked) =>
              onTradingHoursChange(
                checked
                  ? { start_hour: 8, start_minute: 0, end_hour: 16, end_minute: 0 }
                  : undefined
              )
            }
          />
          {tradingHours && (
            <div className="grid grid-cols-2 gap-2">
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">Start</label>
                <div className="flex items-center gap-1">
                  <Input
                    type="number"
                    className="h-8 w-14 text-xs"
                    min={0}
                    max={23}
                    value={tradingHours.start_hour}
                    onChange={(e) => {
                      const v = Math.min(23, Math.max(0, Number(e.target.value) || 0));
                      onTradingHoursChange({ ...tradingHours, start_hour: v });
                    }}
                  />
                  <span className="text-xs text-muted-foreground">:</span>
                  <Input
                    type="number"
                    className="h-8 w-14 text-xs"
                    min={0}
                    max={59}
                    value={tradingHours.start_minute}
                    onChange={(e) => {
                      const v = Math.min(59, Math.max(0, Number(e.target.value) || 0));
                      onTradingHoursChange({ ...tradingHours, start_minute: v });
                    }}
                  />
                </div>
              </div>
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">End</label>
                <div className="flex items-center gap-1">
                  <Input
                    type="number"
                    className="h-8 w-14 text-xs"
                    min={0}
                    max={23}
                    value={tradingHours.end_hour}
                    onChange={(e) => {
                      const v = Math.min(23, Math.max(0, Number(e.target.value) || 0));
                      onTradingHoursChange({ ...tradingHours, end_hour: v });
                    }}
                  />
                  <span className="text-xs text-muted-foreground">:</span>
                  <Input
                    type="number"
                    className="h-8 w-14 text-xs"
                    min={0}
                    max={59}
                    value={tradingHours.end_minute}
                    onChange={(e) => {
                      const v = Math.min(59, Math.max(0, Number(e.target.value) || 0));
                      onTradingHoursChange({ ...tradingHours, end_minute: v });
                    }}
                  />
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Max Daily Trades */}
        <div className="space-y-2">
          <ToggleCheckbox
            label="Limit Daily Trades"
            checked={maxDailyTrades !== undefined}
            onChange={(checked) =>
              onMaxDailyTradesChange(checked ? 5 : undefined)
            }
          />
          {maxDailyTrades !== undefined && (
            <LabeledInput
              label="Max trades per day"
              value={maxDailyTrades}
              onChange={(v) => onMaxDailyTradesChange(Math.max(1, Math.round(v)))}
              min={1}
              step="1"
            />
          )}
        </div>

        {/* Close Trades At */}
        <div className="space-y-2">
          <ToggleCheckbox
            label="Close Trades At"
            checked={!!closeTradesAt}
            onChange={(checked) =>
              onCloseTradesAtChange(
                checked ? { hour: 16, minute: 0 } : undefined
              )
            }
          />
          {closeTradesAt && (
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">Close time</label>
              <div className="flex items-center gap-1">
                <Input
                  type="number"
                  className="h-8 w-14 text-xs"
                  min={0}
                  max={23}
                  value={closeTradesAt.hour}
                  onChange={(e) => {
                    const v = Math.min(23, Math.max(0, Number(e.target.value) || 0));
                    onCloseTradesAtChange({ ...closeTradesAt, hour: v });
                  }}
                />
                <span className="text-xs text-muted-foreground">:</span>
                <Input
                  type="number"
                  className="h-8 w-14 text-xs"
                  min={0}
                  max={59}
                  value={closeTradesAt.minute}
                  onChange={(e) => {
                    const v = Math.min(59, Math.max(0, Number(e.target.value) || 0));
                    onCloseTradesAtChange({ ...closeTradesAt, minute: v });
                  }}
                />
              </div>
            </div>
          )}
        </div>
      </TabsContent>
    </Tabs>
  );
}
