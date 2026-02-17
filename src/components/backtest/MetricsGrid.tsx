import type { BacktestMetrics } from "@/lib/types";
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from "@/components/ui/Tooltip";

interface MetricItem {
  label: string;
  value: string;
  color?: "green" | "red" | "neutral";
  tooltip?: string;
}

interface MetricGroup {
  title: string;
  items: MetricItem[];
}

function fmt(n: number, decimals = 2): string {
  return n.toLocaleString(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

function fmtPct(n: number): string {
  return `${fmt(n, 1)}%`;
}

function colorBySign(n: number): "green" | "red" | "neutral" {
  if (n > 0) return "green";
  if (n < 0) return "red";
  return "neutral";
}

function buildHeroMetrics(m: BacktestMetrics): MetricItem[] {
  return [
    { label: "NET PROFIT", value: `$${fmt(m.net_profit)}`, color: colorBySign(m.net_profit) },
    { label: "WIN RATE", value: fmtPct(m.win_rate_pct), color: m.win_rate_pct >= 50 ? "green" : "red" },
    { label: "PROFIT FACTOR", value: fmt(m.profit_factor), color: m.profit_factor >= 1 ? "green" : "red" },
    { label: "MAX DRAWDOWN", value: fmtPct(m.max_drawdown_pct), color: "red" },
  ];
}

function buildGroups(m: BacktestMetrics): MetricGroup[] {
  return [
    {
      title: "RETURNS",
      items: [
        { label: "FINAL CAPITAL", value: `$${fmt(m.final_capital)}`, color: colorBySign(m.total_return_pct) },
        { label: "TOTAL RETURN", value: fmtPct(m.total_return_pct), color: colorBySign(m.total_return_pct) },
        { label: "ANNUALIZED", value: fmtPct(m.annualized_return_pct), color: colorBySign(m.annualized_return_pct) },
        { label: "MONTHLY AVG", value: fmtPct(m.monthly_return_avg_pct), color: colorBySign(m.monthly_return_avg_pct) },
      ],
    },
    {
      title: "RISK-ADJUSTED",
      items: [
        { label: "SHARPE", value: fmt(m.sharpe_ratio), color: colorBySign(m.sharpe_ratio), tooltip: "Risk-adjusted return (annualized). Higher = better" },
        { label: "SORTINO", value: fmt(m.sortino_ratio), color: colorBySign(m.sortino_ratio), tooltip: "Like Sharpe but only penalizes downside volatility" },
        { label: "CALMAR", value: fmt(m.calmar_ratio), color: colorBySign(m.calmar_ratio), tooltip: "Annualized return / Max drawdown" },
        { label: "RET/DD", value: fmt(m.return_dd_ratio), color: colorBySign(m.return_dd_ratio), tooltip: "Total return % / Max drawdown %. Higher = better" },
      ],
    },
    {
      title: "DRAWDOWN",
      items: [
        { label: "MAX DD", value: fmtPct(m.max_drawdown_pct), color: "red" },
        { label: "MAX DD DURATION", value: m.max_drawdown_duration_time },
        { label: "AVG DD", value: fmtPct(m.avg_drawdown_pct) },
        { label: "RECOVERY FACTOR", value: fmt(m.recovery_factor), tooltip: "Net profit / Max drawdown" },
      ],
    },
    {
      title: "TRADES",
      items: [
        { label: "TOTAL", value: String(m.total_trades) },
        { label: "WINNERS", value: String(m.winning_trades), color: "green" },
        { label: "LOSERS", value: String(m.losing_trades), color: "red" },
        { label: "BREAKEVEN", value: String(m.breakeven_trades) },
      ],
    },
    {
      title: "P&L",
      items: [
        { label: "GROSS PROFIT", value: `$${fmt(m.gross_profit)}`, color: "green" },
        { label: "GROSS LOSS", value: `$${fmt(m.gross_loss)}`, color: "red" },
        { label: "AVG TRADE", value: `$${fmt(m.avg_trade)}`, color: colorBySign(m.avg_trade) },
        { label: "AVG WIN", value: `$${fmt(m.avg_win)}`, color: "green" },
        { label: "AVG LOSS", value: `$${fmt(m.avg_loss)}`, color: "red" },
        { label: "LARGEST WIN", value: `$${fmt(m.largest_win)}`, color: "green" },
        { label: "LARGEST LOSS", value: `$${fmt(m.largest_loss)}`, color: "red" },
        { label: "EXPECTANCY", value: `$${fmt(m.expectancy)}`, color: colorBySign(m.expectancy), tooltip: "Average $ expected per trade" },
      ],
    },
    {
      title: "CONSISTENCY",
      items: [
        { label: "MAX CONSEC WINS", value: String(m.max_consecutive_wins) },
        { label: "MAX CONSEC LOSSES", value: String(m.max_consecutive_losses) },
        { label: "AVG CONSEC WINS", value: fmt(m.avg_consecutive_wins, 1) },
        { label: "AVG CONSEC LOSSES", value: fmt(m.avg_consecutive_losses, 1) },
      ],
    },
    {
      title: "RISK",
      items: [
        { label: "MAE AVG", value: fmt(m.mae_avg, 1), tooltip: "Max Adverse Excursion — avg worst drawdown during a trade" },
        { label: "MAE MAX", value: fmt(m.mae_max, 1), tooltip: "Worst single-trade drawdown" },
        { label: "MFE AVG", value: fmt(m.mfe_avg, 1), tooltip: "Max Favorable Excursion — avg best peak during a trade" },
        { label: "MFE MAX", value: fmt(m.mfe_max, 1), tooltip: "Best single-trade peak" },
        { label: "STAGNATION", value: m.stagnation_time, tooltip: "Longest period without a new equity high" },
        { label: "ULCER INDEX", value: fmtPct(m.ulcer_index_pct), tooltip: "Root-mean-square of drawdown percentages — lower is better" },
      ],
    },
  ];
}

const VALUE_COLORS = {
  green: "text-emerald-400",
  red: "text-red-400",
  neutral: "text-foreground",
};

interface MetricsGridProps {
  metrics: BacktestMetrics;
}

export function MetricsGrid({ metrics }: MetricsGridProps) {
  const heroMetrics = buildHeroMetrics(metrics);
  const groups = buildGroups(metrics);

  return (
    <div className="space-y-5">
      {/* Hero row — 4 key metrics */}
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        {heroMetrics.map((item) => (
          <div
            key={item.label}
            className="flex flex-col items-center justify-center rounded-md border border-border bg-background px-4 py-5"
          >
            <p
              className={`text-xl font-bold tabular-nums ${
                item.color ? VALUE_COLORS[item.color] : ""
              }`}
            >
              {item.value}
            </p>
            <p className="mt-1.5 text-[10px] font-medium tracking-widest text-muted-foreground">
              {item.label}
            </p>
          </div>
        ))}
      </div>

      {/* Detailed groups */}
      {groups.map((group) => (
        <div key={group.title}>
          <h4 className="mb-2 text-[10px] font-semibold tracking-[0.15em] text-muted-foreground">
            {group.title}
          </h4>
          <div className="grid grid-cols-2 gap-2 md:grid-cols-4 lg:grid-cols-4">
            {group.items.map((item) => (
              <div
                key={item.label}
                className="flex flex-col items-center justify-center rounded-md border border-border/60 bg-card px-3 py-3"
              >
                <p
                  className={`text-sm font-semibold tabular-nums ${
                    item.color ? VALUE_COLORS[item.color] : ""
                  }`}
                >
                  {item.value}
                </p>
                {item.tooltip ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <p className="mt-1 cursor-help text-[10px] tracking-widest text-muted-foreground underline decoration-dotted underline-offset-2">
                        {item.label}
                      </p>
                    </TooltipTrigger>
                    <TooltipContent side="top" className="max-w-56 text-xs">
                      {item.tooltip}
                    </TooltipContent>
                  </Tooltip>
                ) : (
                  <p className="mt-1 text-[10px] tracking-widest text-muted-foreground">
                    {item.label}
                  </p>
                )}
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
