import { useState, useMemo } from "react";
import {
  ArrowLeft, Code2, ExternalLink, Settings2, BarChart3, ChevronRight, TestTube2, BookmarkCheck,
} from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import {
  ResponsiveContainer, ComposedChart, Area, XAxis, YAxis, CartesianGrid, Tooltip, ReferenceLine,
} from "recharts";
import { cn } from "@/lib/utils";
import { getChartTheme, CHART_COLORS } from "@/lib/chartTheme";
import type { BuilderSavedStrategy, Strategy, Rule, RuleGroup, Operand } from "@/lib/types";

// ── Operand → readable string ─────────────────────────────────────────────────

function operandToString(op: Operand): string {
  if (op.operand_type === "Indicator" && op.indicator) {
    const { indicator_type, params, output_field } = op.indicator;
    const paramStr = Object.entries(params)
      .filter(([, v]) => v !== null && v !== undefined)
      .map(([, v]) => String(v))
      .join(", ");
    const field = output_field ? `.${output_field}` : "";
    const offset = op.offset ? `[${op.offset}]` : "";
    return `${indicator_type}(${paramStr})${field}${offset}`;
  }
  if (op.operand_type === "Price") return op.price_field ?? "Price";
  if (op.operand_type === "Constant") return String(op.constant_value ?? 0);
  if (op.operand_type === "Compound" && op.compound_left && op.compound_right) {
    const sym: Record<string, string> = { Add: "+", Sub: "−", Mul: "×", Div: "÷" };
    return `(${operandToString(op.compound_left)} ${sym[op.compound_op ?? "Add"] ?? "?"} ${operandToString(op.compound_right)})`;
  }
  if (op.operand_type === "CandlePattern") return op.candle_pattern ?? "Pattern";
  return "?";
}

const CMP: Record<string, string> = {
  GreaterThan: ">", LessThan: "<", GreaterOrEqual: "≥",
  LessOrEqual: "≤", Equal: "=", CrossAbove: "↑ crosses", CrossBelow: "↓ crosses",
};

// ── Rule row ──────────────────────────────────────────────────────────────────

function RuleRow({ rule, idx }: { rule: Rule; idx: number }) {
  const l = operandToString(rule.left_operand);
  const c = CMP[rule.comparator] ?? rule.comparator;
  const r = operandToString(rule.right_operand);
  return (
    <div className="flex items-start gap-2 py-1.5 border-b border-border/10 last:border-0 text-[11px]">
      <span className="mt-0.5 shrink-0 rounded bg-primary/10 px-1.5 text-[9px] font-bold text-primary tabular-nums">
        {idx + 1}
      </span>
      <span className="font-mono break-all leading-relaxed">
        <span className="text-sky-400">{l}</span>
        <span className="mx-1.5 text-muted-foreground/50">{c}</span>
        <span className="text-amber-400">{r}</span>
      </span>
      {rule.logical_operator && (
        <span className="ml-auto shrink-0 rounded bg-muted/40 px-1.5 py-0.5 text-[9px] font-bold text-muted-foreground">
          {rule.logical_operator}
        </span>
      )}
    </div>
  );
}

function GroupBlock({ group, gIdx, isLast }: { group: RuleGroup; gIdx: number; isLast: boolean }) {
  return (
    <div className="space-y-0">
      {group.rules.map((r, i) => <RuleRow key={r.id} rule={r} idx={i} />)}
      {!isLast && group.join && (
        <div className="flex items-center gap-2 py-1">
          <div className="h-px flex-1 bg-border/20" />
          <span className="rounded bg-primary/10 px-2 py-0.5 text-[9px] font-bold text-primary">
            {group.join} (grupos {gIdx + 1}–{gIdx + 2})
          </span>
          <div className="h-px flex-1 bg-border/20" />
        </div>
      )}
    </div>
  );
}

function RulesBlock({ label, rules, groups }: { label: string; rules: Rule[]; groups?: RuleGroup[] }) {
  const hasGroups = groups && groups.length > 0;
  const hasFlat = rules.length > 0;
  if (!hasGroups && !hasFlat) return null;
  return (
    <div className="space-y-1">
      <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">{label}</p>
      <div className="rounded border border-border/30 bg-muted/5 px-3">
        {hasGroups
          ? groups!.map((g, gi) => (
              <GroupBlock key={g.id} group={g} gIdx={gi} isLast={gi === groups!.length - 1} />
            ))
          : rules.map((r, i) => <RuleRow key={r.id} rule={r} idx={i} />)
        }
      </div>
    </div>
  );
}

function InfoRow({ label, value, accent }: { label: string; value: string; accent?: string }) {
  return (
    <div className="flex items-center justify-between gap-3 py-0.5 text-[11px]">
      <span className="text-muted-foreground/60 shrink-0">{label}</span>
      <span className={cn("font-medium text-right", accent ?? "text-foreground")}>{value}</span>
    </div>
  );
}

function InfoCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded border border-border/30 bg-muted/5 p-3 space-y-0.5">
      <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">{title}</p>
      {children}
    </div>
  );
}

// ── Left panel: Configuration ─────────────────────────────────────────────────

function ConfigPanel({ strategy }: { strategy: Strategy }) {
  const sl = strategy.stop_loss;
  const tp = strategy.take_profit;
  const ts = strategy.trailing_stop;
  const sz = strategy.position_sizing;

  const slLabel = sl ? `${sl.value} ${sl.sl_type}${sl.atr_period ? ` · ATR ${sl.atr_period}` : ""}` : "None";
  const tpLabel = tp ? `${tp.value} ${tp.tp_type}${tp.atr_period ? ` · ATR ${tp.atr_period}` : ""}` : "None";
  const tsLabel = ts ? `${ts.value} ${ts.ts_type}${ts.atr_period ? ` · ATR ${ts.atr_period}` : ""}` : "None";

  const szTypes: Record<string, string> = {
    FixedLots: "Fixed Lots", FixedAmount: "Fixed Amount",
    PercentEquity: "% Equity", RiskBased: "Risk Based", AntiMartingale: "Anti-Martingale",
  };

  return (
    <div className="h-full overflow-y-auto space-y-4 p-4">
      {/* Header */}
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-xs font-semibold text-foreground">{strategy.name}</span>
        <span className="rounded bg-muted/40 px-1.5 py-0.5 text-[10px] text-muted-foreground/60">
          {strategy.trade_direction}
        </span>
        <span className="rounded bg-muted/40 px-1.5 py-0.5 text-[10px] text-muted-foreground/60">
          {[
            ...(strategy.long_entry_groups?.flatMap(g => g.rules) ?? strategy.long_entry_rules),
            ...(strategy.short_entry_groups?.flatMap(g => g.rules) ?? strategy.short_entry_rules),
            ...(strategy.long_exit_groups?.flatMap(g => g.rules) ?? strategy.long_exit_rules),
            ...(strategy.short_exit_groups?.flatMap(g => g.rules) ?? strategy.short_exit_rules),
          ].length} rules
        </span>
      </div>

      <RulesBlock label="Long Entry"  rules={strategy.long_entry_rules}  groups={strategy.long_entry_groups} />
      <RulesBlock label="Short Entry" rules={strategy.short_entry_rules} groups={strategy.short_entry_groups} />
      <RulesBlock label="Long Exit"   rules={strategy.long_exit_rules}   groups={strategy.long_exit_groups} />
      <RulesBlock label="Short Exit"  rules={strategy.short_exit_rules}  groups={strategy.short_exit_groups} />

      <InfoCard title="Stop Loss / Take Profit / Trailing">
        <InfoRow label="Stop Loss"     value={slLabel} accent={sl ? "text-red-400"     : undefined} />
        <InfoRow label="Take Profit"   value={tpLabel} accent={tp ? "text-emerald-400" : undefined} />
        <InfoRow label="Trailing Stop" value={tsLabel} accent={ts ? "text-sky-400"     : undefined} />
      </InfoCard>

      <InfoCard title="Position Sizing">
        <InfoRow label="Type"  value={szTypes[sz.sizing_type] ?? sz.sizing_type} />
        <InfoRow label="Value" value={String(sz.value)} />
        {sz.decrease_factor != null && (
          <InfoRow label="Decrease factor" value={String(sz.decrease_factor)} />
        )}
      </InfoCard>

      <InfoCard title="Costs">
        <InfoRow label="Spread"     value={`${strategy.trading_costs.spread_pips} pips`} />
        <InfoRow
          label="Commission"
          value={`${strategy.trading_costs.commission_value} ${strategy.trading_costs.commission_type === "Percentage" ? "%" : "$/lot"}`}
        />
        <InfoRow label="Slippage" value={`${strategy.trading_costs.slippage_pips} pips`} />
      </InfoCard>
    </div>
  );
}

// ── Right panel: Results (from saved data, instant) ───────────────────────────

function MetricCard({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div className="flex flex-col gap-0.5 rounded border border-border/30 bg-muted/5 px-3 py-2">
      <span className="text-[9px] uppercase tracking-wider text-muted-foreground/50">{label}</span>
      <span className={cn("text-sm font-bold tabular-nums", color ?? "text-foreground")}>{value}</span>
    </div>
  );
}

function EquityChart({ data, initialCapital }: { data: number[]; initialCapital: number }) {
  const { GRID_COLOR, GRID_DASH, AXIS_TICK, AXIS_STROKE, TOOLTIP_STYLE } = getChartTheme();
  const points = useMemo(() => data.map((v, i) => ({ i, v })), [data]);
  if (points.length < 2) return (
    <div className="flex h-32 items-center justify-center text-xs text-muted-foreground/30">No data</div>
  );

  const minV = Math.min(...data);
  const maxV = Math.max(...data);
  const range = maxV - minV;
  const splitOffset = range > 0 ? Math.max(0, Math.min(1, (maxV - initialCapital) / range)) : 0.5;
  const allAbove = minV >= initialCapital;
  const allBelow = maxV <= initialCapital;

  return (
    <ResponsiveContainer width="100%" height={220}>
      <ComposedChart data={points} margin={{ top: 5, right: 16, bottom: 4, left: 8 }}>
        <defs>
          <linearGradient id="miniStroke" x1="0" y1="0" x2="0" y2="1">
            {allBelow ? (
              <><stop offset="0%" stopColor={CHART_COLORS.red} /><stop offset="100%" stopColor={CHART_COLORS.red} /></>
            ) : allAbove ? (
              <><stop offset="0%" stopColor={CHART_COLORS.green} /><stop offset="100%" stopColor={CHART_COLORS.green} /></>
            ) : (
              <>
                <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.green} />
                <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.red} />
              </>
            )}
          </linearGradient>
          <linearGradient id="miniFill" x1="0" y1="0" x2="0" y2="1">
            {allBelow ? (
              <><stop offset="0%" stopColor={CHART_COLORS.red} stopOpacity={0.25} /><stop offset="100%" stopColor={CHART_COLORS.red} stopOpacity={0.03} /></>
            ) : allAbove ? (
              <><stop offset="0%" stopColor={CHART_COLORS.green} stopOpacity={0.3} /><stop offset="100%" stopColor={CHART_COLORS.green} stopOpacity={0.03} /></>
            ) : (
              <>
                <stop offset="0%" stopColor={CHART_COLORS.green} stopOpacity={0.3} />
                <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.green} stopOpacity={0.1} />
                <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.red} stopOpacity={0.1} />
                <stop offset="100%" stopColor={CHART_COLORS.red} stopOpacity={0.03} />
              </>
            )}
          </linearGradient>
        </defs>
        <CartesianGrid strokeDasharray={GRID_DASH} stroke={GRID_COLOR} />
        <XAxis dataKey="i" hide />
        <YAxis
          domain={["auto", "auto"]}
          tickFormatter={(v: number) => `$${v.toLocaleString()}`}
          tick={AXIS_TICK}
          stroke={AXIS_STROKE}
          width={72}
        />
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          formatter={(v: number) => [`$${v.toLocaleString(undefined, { maximumFractionDigits: 2 })}`, "Equity"]}
          labelFormatter={() => ""}
        />
        {!allAbove && !allBelow && (
          <ReferenceLine y={initialCapital} stroke="hsl(0 0% 35%)" strokeDasharray="4 4" strokeWidth={1} />
        )}
        <Area
          type="monotone"
          dataKey="v"
          stroke="url(#miniStroke)"
          fill="url(#miniFill)"
          strokeWidth={1.5}
          dot={false}
          animationDuration={400}
        />
      </ComposedChart>
    </ResponsiveContainer>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
      {children}
    </p>
  );
}

function ResultsPanel({ saved }: { saved: BuilderSavedStrategy }) {
  const fmt = (n: number, d = 2) =>
    n.toLocaleString(undefined, { minimumFractionDigits: d, maximumFractionDigits: d });
  const money = (n: number) =>
    (n >= 0 ? "+" : "") + "$" + Math.abs(n).toLocaleString(undefined, { maximumFractionDigits: 0 });

  const hasOos = saved.oosNetProfit !== undefined;

  return (
    <div className="h-full overflow-y-auto space-y-5 p-4">
      {/* Equity curve */}
      <div className="rounded border border-border/30 bg-muted/5 p-3">
        <SectionTitle>Equity Curve (In-Sample)</SectionTitle>
        <EquityChart
          data={saved.miniEquityCurve}
          initialCapital={saved.miniEquityCurve.length > 0 ? saved.miniEquityCurve[0] : 10000}
        />
      </div>

      {/* Performance */}
      <div>
        <SectionTitle>Performance</SectionTitle>
        <div className="grid grid-cols-4 gap-2">
          <MetricCard label="Net Profit"    value={money(saved.netProfit)}
            color={saved.netProfit >= 0 ? "text-emerald-400" : "text-red-400"} />
          <MetricCard label="Annual Return" value={fmt(saved.annualReturnPct) + "%"}
            color={saved.annualReturnPct >= 0 ? "text-emerald-400" : "text-red-400"} />
          <MetricCard label="Profit Factor" value={fmt(saved.profitFactor)}
            color={saved.profitFactor >= 1.5 ? "text-emerald-400" : saved.profitFactor < 1 ? "text-red-400" : undefined} />
          <MetricCard label="Fitness"       value={saved.fitness.toFixed(4)} color="text-primary" />
        </div>
      </div>

      {/* Risk */}
      <div>
        <SectionTitle>Risk</SectionTitle>
        <div className="grid grid-cols-4 gap-2">
          <MetricCard label="Sharpe Ratio"  value={fmt(saved.sharpeRatio)}
            color={saved.sharpeRatio >= 1 ? "text-emerald-400" : undefined} />
          <MetricCard label="Max Drawdown"  value={"$" + saved.maxDrawdownAbs.toLocaleString(undefined, { maximumFractionDigits: 0 })} color="text-red-400" />
          <MetricCard label="Ret/DD Ratio"  value={fmt(saved.retDDRatio)}
            color={saved.retDDRatio >= 2 ? "text-emerald-400" : undefined} />
          <MetricCard label="CAGR/MaxDD"    value={fmt(saved.cagrMaxDDPct)}
            color={saved.cagrMaxDDPct >= 1 ? "text-emerald-400" : undefined} />
        </div>
      </div>

      {/* Trade stats */}
      <div>
        <SectionTitle>Trade Stats</SectionTitle>
        <div className="grid grid-cols-4 gap-2">
          <MetricCard label="# Trades"       value={saved.trades.toLocaleString()} />
          <MetricCard label="Win Rate"        value={fmt(saved.winRatePct ?? 0) + "%"}
            color={(saved.winRatePct ?? 0) >= 50 ? "text-emerald-400" : undefined} />
          <MetricCard label="Avg Win"         value={"$" + fmt(saved.avgWin, 0)}       color="text-emerald-400" />
          <MetricCard label="Avg Loss"        value={"-$" + fmt(Math.abs(saved.avgLoss), 0)} color="text-red-400" />
          <MetricCard label="Win/Loss Ratio"  value={fmt(saved.winLossRatio)} />
          <MetricCard label="R Expectancy"    value={fmt(saved.rExpectancy)}
            color={saved.rExpectancy >= 0 ? "text-emerald-400" : "text-red-400"} />
          <MetricCard label="Avg Bars Win"    value={fmt(saved.avgBarsWin, 1)} />
        </div>
      </div>

      {/* OOS section */}
      {hasOos && (
        <div>
          <SectionTitle>Out-of-Sample Validation</SectionTitle>
          <div className="rounded border border-primary/20 bg-primary/5 p-3 space-y-2">
            <div className="grid grid-cols-4 gap-2">
              <MetricCard label="OOS Net Profit"    value={money(saved.oosNetProfit!)}
                color={saved.oosNetProfit! >= 0 ? "text-emerald-400" : "text-red-400"} />
              <MetricCard label="OOS Trades"        value={(saved.oosTrades ?? 0).toLocaleString()} />
              <MetricCard label="OOS Profit Factor" value={fmt(saved.oosProfitFactor ?? 0)}
                color={(saved.oosProfitFactor ?? 0) >= 1.5 ? "text-emerald-400" : (saved.oosProfitFactor ?? 0) < 1 ? "text-red-400" : undefined} />
              <MetricCard label="OOS Sharpe"        value={fmt(saved.oosSharpeRatio ?? 0)}
                color={(saved.oosSharpeRatio ?? 0) >= 1 ? "text-emerald-400" : undefined} />
              <MetricCard label="OOS Win Rate"      value={fmt(saved.oosWinRatePct ?? 0) + "%"}
                color={(saved.oosWinRatePct ?? 0) >= 50 ? "text-emerald-400" : undefined} />
              <MetricCard label="OOS Max DD"        value={"$" + (saved.oosMaxDrawdownAbs ?? 0).toLocaleString(undefined, { maximumFractionDigits: 0 })} color="text-red-400" />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Main overlay ──────────────────────────────────────────────────────────────

type DetailTab = "results" | "config";

interface StrategyDetailOverlayProps {
  saved: BuilderSavedStrategy;
  onClose: () => void;
}

export function StrategyDetailOverlay({ saved, onClose }: StrategyDetailOverlayProps) {
  const { setCurrentStrategy, setActiveSection, setPendingBacktestRun, saveStrategyToResults, setBuilderTopTab } = useAppStore();
  const [activeTab, setActiveTab] = useState<DetailTab>("results");
  const [savedToResults, setSavedToResults] = useState(false);

  const strategy = useMemo<Strategy | null>(() => {
    try { return JSON.parse(saved.strategyJson) as Strategy; }
    catch { return null; }
  }, [saved.strategyJson]);

  const handleLoadInBuilder = () => {
    if (strategy) { setCurrentStrategy(strategy); setActiveSection("strategy"); }
  };

  const handleExport = () => {
    if (strategy) { setCurrentStrategy(strategy); setActiveSection("export"); }
  };

  const handleSendToBacktest = () => {
    if (strategy) {
      setCurrentStrategy(strategy);
      setPendingBacktestRun(true);
      setActiveSection("backtest");
    }
  };

  const handleSaveToResults = () => {
    saveStrategyToResults(saved);
    setSavedToResults(true);
    setTimeout(() => setSavedToResults(false), 2000);
  };

  const handleGoToResults = () => {
    saveStrategyToResults(saved);
    onClose();
    setBuilderTopTab("results");
  };

  const TABS: { id: DetailTab; label: string; icon: React.ReactNode }[] = [
    { id: "results", label: "Resultados",          icon: <BarChart3 className="h-3.5 w-3.5" /> },
    { id: "config",  label: "Configuración Completa", icon: <Settings2 className="h-3.5 w-3.5" /> },
  ];

  return (
    <div className="flex h-full flex-col bg-background">

      {/* ── Top bar ─────────────────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-3 border-b border-border/40 bg-muted/5 px-4 py-2.5">
        <button
          onClick={onClose}
          className="flex items-center gap-1.5 rounded border border-border/30 px-3 py-1.5 text-xs text-muted-foreground hover:border-border/60 hover:text-foreground"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          Volver
        </button>

        <ChevronRight className="h-3.5 w-3.5 text-muted-foreground/30" />

        <div className="flex min-w-0 flex-col">
          <span className="truncate text-sm font-semibold text-foreground">{saved.name}</span>
          <span className="text-[10px] text-muted-foreground/60">
            {saved.symbolName} · {saved.timeframe.toUpperCase()} ·{" "}
            <span className={cn("font-medium", saved.netProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
              {saved.netProfit >= 0 ? "+" : ""}${Math.abs(saved.netProfit).toLocaleString(undefined, { maximumFractionDigits: 0 })}
            </span>
            {" · fitness "}
            <span className="font-medium text-primary">{saved.fitness.toFixed(4)}</span>
          </span>
        </div>

        <div className="ml-auto flex items-center gap-2">
          <button
            onClick={handleLoadInBuilder}
            disabled={!strategy}
            className="flex items-center gap-1.5 rounded border border-border/40 px-3 py-1.5 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary disabled:opacity-40"
          >
            <Settings2 className="h-3.5 w-3.5" />
            Cargar en Builder
          </button>
          <button
            onClick={handleSendToBacktest}
            disabled={!strategy}
            className="flex items-center gap-1.5 rounded border border-border/40 px-3 py-1.5 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary disabled:opacity-40"
          >
            <TestTube2 className="h-3.5 w-3.5" />
            Probar en Backtest
          </button>
          <button
            onClick={handleGoToResults}
            className={cn(
              "flex items-center gap-1.5 rounded border px-3 py-1.5 text-xs font-medium transition-colors",
              savedToResults
                ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-400"
                : "border-emerald-600/30 bg-emerald-600/10 text-emerald-400 hover:bg-emerald-600/20"
            )}
            title="Guardar en databank Resultados y ver análisis completo"
          >
            <BookmarkCheck className="h-3.5 w-3.5" />
            {savedToResults ? "¡Guardado!" : "Guardar en Resultados"}
          </button>
          <button
            onClick={handleExport}
            disabled={!strategy}
            className="flex items-center gap-1.5 rounded border border-primary/30 bg-primary/10 px-3 py-1.5 text-xs text-primary hover:bg-primary/20 disabled:opacity-40"
          >
            <Code2 className="h-3.5 w-3.5" />
            Exportar Código
            <ExternalLink className="h-3 w-3 opacity-60" />
          </button>
        </div>
      </div>

      {/* ── Tabs ─────────────────────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-1 border-b border-border/30 bg-muted/5 px-3 py-1.5">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            disabled={tab.id === "config" && !strategy}
            className={cn(
              "flex items-center gap-1.5 rounded px-3 py-1.5 text-xs font-medium transition-colors",
              activeTab === tab.id
                ? "border border-primary/30 bg-primary/10 text-primary"
                : "border border-transparent text-muted-foreground hover:border-border/40 hover:text-foreground",
              "disabled:cursor-not-allowed disabled:opacity-40"
            )}
          >
            {tab.icon}
            {tab.label}
          </button>
        ))}
      </div>

      {/* ── Tab body ─────────────────────────────────────────────────────── */}
      <div className="min-h-0 flex-1 overflow-auto">
        {activeTab === "results" && <ResultsPanel saved={saved} />}
        {activeTab === "config" && strategy && <ConfigPanel strategy={strategy} />}
        {activeTab === "config" && !strategy && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground/40">
            No se pudo parsear la estrategia
          </div>
        )}
      </div>
    </div>
  );
}
