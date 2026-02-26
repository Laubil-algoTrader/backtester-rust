import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import type { BacktestMetrics } from "@/lib/types";
import {
  Tooltip,
  TooltipTrigger,
  TooltipContent,
} from "@/components/ui/Tooltip";
import { cn } from "@/lib/utils";

interface MetricItem {
  label: string;
  value: string;
  color?: "green" | "red" | "neutral";
  tooltip?: string;
}

interface MetricGroup {
  title: string;
  items: MetricItem[];
  wide?: boolean; // use 2-col internal layout
  fullWidth?: boolean; // span full grid width
}

interface HeroMetric {
  label: string;
  value: string;
  color: "green" | "red" | "neutral";
  badge?: string;
  badgeColor?: "green" | "red";
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

function buildHeroMetrics(m: BacktestMetrics, t: TFunction): HeroMetric[] {
  return [
    {
      label: t("metrics.netProfit"),
      value: `$${fmt(m.net_profit)}`,
      color: colorBySign(m.net_profit),
      badge: fmtPct(m.total_return_pct),
      badgeColor: m.total_return_pct >= 0 ? "green" : "red",
    },
    {
      label: t("metrics.winRate"),
      value: fmtPct(m.win_rate_pct),
      color: m.win_rate_pct >= 50 ? "green" : "red",
      badge: `${m.winning_trades}/${m.total_trades}`,
      badgeColor: m.win_rate_pct >= 50 ? "green" : "red",
    },
    {
      label: t("metrics.profitFactor"),
      value: fmt(m.profit_factor),
      color: m.profit_factor >= 1 ? "green" : "red",
      badge: `$${fmt(m.avg_trade)}`,
      badgeColor: m.avg_trade >= 0 ? "green" : "red",
    },
    {
      label: t("metrics.maxDrawdown"),
      value: fmtPct(m.max_drawdown_pct),
      color: "red",
      badge: m.max_drawdown_duration_time,
      badgeColor: "red",
    },
    {
      label: t("metrics.sharpeRatio"),
      value: fmt(m.sharpe_ratio),
      color: colorBySign(m.sharpe_ratio),
      badge: `${t("metrics.sortino")}: ${fmt(m.sortino_ratio)}`,
      badgeColor: m.sortino_ratio >= 0 ? "green" : "red",
    },
  ];
}

function buildGroups(m: BacktestMetrics, t: TFunction): MetricGroup[] {
  return [
    {
      title: t("metrics.returns"),
      items: [
        { label: t("metrics.finalCapital"), value: `$${fmt(m.final_capital)}`, color: colorBySign(m.total_return_pct) },
        { label: t("metrics.totalReturn"), value: fmtPct(m.total_return_pct), color: colorBySign(m.total_return_pct) },
        { label: t("metrics.annualized"), value: fmtPct(m.annualized_return_pct), color: colorBySign(m.annualized_return_pct) },
        { label: t("metrics.monthlyAvg"), value: fmtPct(m.monthly_return_avg_pct), color: colorBySign(m.monthly_return_avg_pct) },
      ],
    },
    {
      title: t("metrics.riskAdjusted"),
      items: [
        { label: t("metrics.sharpe"), value: fmt(m.sharpe_ratio), color: colorBySign(m.sharpe_ratio), tooltip: t("tooltips.sharpe") },
        { label: t("metrics.sortino"), value: fmt(m.sortino_ratio), color: colorBySign(m.sortino_ratio), tooltip: t("tooltips.sortino") },
        { label: t("metrics.calmar"), value: fmt(m.calmar_ratio), color: colorBySign(m.calmar_ratio), tooltip: t("tooltips.calmar") },
        { label: t("metrics.retDd"), value: fmt(m.return_dd_ratio), color: colorBySign(m.return_dd_ratio), tooltip: t("tooltips.retDd") },
      ],
    },
    {
      title: t("metrics.drawdownGroup"),
      items: [
        { label: t("metrics.maxDd"), value: fmtPct(m.max_drawdown_pct), color: "red" },
        { label: t("metrics.maxDdDuration"), value: m.max_drawdown_duration_time },
        { label: t("metrics.avgDd"), value: fmtPct(m.avg_drawdown_pct) },
        { label: t("metrics.recoveryFactor"), value: fmt(m.recovery_factor), tooltip: t("tooltips.recoveryFactor") },
      ],
    },
    {
      title: t("metrics.tradesGroup"),
      items: [
        { label: t("metrics.totalTrades"), value: String(m.total_trades) },
        { label: t("metrics.winners"), value: String(m.winning_trades), color: "green" },
        { label: t("metrics.losers"), value: String(m.losing_trades), color: "red" },
        { label: t("metrics.breakeven"), value: String(m.breakeven_trades) },
      ],
    },
    {
      title: t("metrics.pnlGroup"),
      wide: true,
      items: [
        { label: t("metrics.grossProfit"), value: `$${fmt(m.gross_profit)}`, color: "green" },
        { label: t("metrics.grossLoss"), value: `$${fmt(m.gross_loss)}`, color: "red" },
        { label: t("metrics.avgTrade"), value: `$${fmt(m.avg_trade)}`, color: colorBySign(m.avg_trade) },
        { label: t("metrics.avgWin"), value: `$${fmt(m.avg_win)}`, color: "green" },
        { label: t("metrics.avgLoss"), value: `$${fmt(m.avg_loss)}`, color: "red" },
        { label: t("metrics.largestWin"), value: `$${fmt(m.largest_win)}`, color: "green" },
        { label: t("metrics.largestLoss"), value: `$${fmt(m.largest_loss)}`, color: "red" },
        { label: t("metrics.expectancy"), value: `$${fmt(m.expectancy)}`, color: colorBySign(m.expectancy), tooltip: t("tooltips.expectancy") },
      ],
    },
    {
      title: t("metrics.consistency"),
      items: [
        { label: t("metrics.maxConsecWins"), value: String(m.max_consecutive_wins) },
        { label: t("metrics.maxConsecLosses"), value: String(m.max_consecutive_losses) },
        { label: t("metrics.avgConsecWins"), value: fmt(m.avg_consecutive_wins, 1) },
        { label: t("metrics.avgConsecLosses"), value: fmt(m.avg_consecutive_losses, 1) },
      ],
    },
    {
      title: t("metrics.riskAnalytics"),
      wide: true,
      fullWidth: true,
      items: [
        { label: t("metrics.maeAvg"), value: fmt(m.mae_avg, 1), tooltip: t("tooltips.maeAvg") },
        { label: t("metrics.maeMax"), value: fmt(m.mae_max, 1), tooltip: t("tooltips.maeMax") },
        { label: t("metrics.mfeAvg"), value: fmt(m.mfe_avg, 1), tooltip: t("tooltips.mfeAvg") },
        { label: t("metrics.mfeMax"), value: fmt(m.mfe_max, 1), tooltip: t("tooltips.mfeMax") },
        { label: t("metrics.stagnation"), value: m.stagnation_time, tooltip: t("tooltips.stagnation") },
        { label: t("metrics.ulcerIndex"), value: fmtPct(m.ulcer_index_pct), tooltip: t("tooltips.ulcerIndex") },
      ],
    },
  ];
}

const VALUE_COLORS = {
  green: "text-emerald-400",
  red: "text-red-400",
  neutral: "text-foreground",
};

const BADGE_COLORS = {
  green: "bg-emerald-500/15 text-emerald-400",
  red: "bg-red-500/15 text-red-400",
};

function MetricLabel({ item }: { item: MetricItem }) {
  if (item.tooltip) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="cursor-help text-xs text-muted-foreground underline decoration-dotted underline-offset-2">
            {item.label}
          </span>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-56 text-sm">
          {item.tooltip}
        </TooltipContent>
      </Tooltip>
    );
  }
  return <span className="text-xs text-muted-foreground">{item.label}</span>;
}

interface MetricsGridProps {
  metrics: BacktestMetrics;
}

export function MetricsGrid({ metrics }: MetricsGridProps) {
  const { t } = useTranslation("backtest");
  const heroMetrics = buildHeroMetrics(metrics, t);
  const groups = buildGroups(metrics, t);

  return (
    <div className="space-y-3">
      {/* Hero row — 5 key metrics */}
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 lg:grid-cols-5">
        {heroMetrics.map((item) => (
          <div
            key={item.label}
            className="rounded-md border border-border/30 bg-card p-4"
          >
            <p className="text-xs text-muted-foreground">{item.label}</p>
            <div className="mt-1 flex items-baseline gap-2">
              <span
                className={cn(
                  "text-xl font-mono tabular-nums",
                  VALUE_COLORS[item.color],
                )}
              >
                {item.value}
              </span>
              {item.badge && (
                <span
                  className={cn(
                    "rounded-full px-1.5 py-0.5 text-[10px] font-mono",
                    item.badgeColor ? BADGE_COLORS[item.badgeColor] : "bg-foreground/10 text-muted-foreground",
                  )}
                >
                  {item.badge}
                </span>
              )}
            </div>
          </div>
        ))}
      </div>

      {/* Metric group cards */}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        {groups.map((group) => (
          <div
            key={group.title}
            className={cn(
              "rounded-md border border-border/30 bg-card p-3",
              group.fullWidth && "md:col-span-2",
            )}
          >
            <h4 className="mb-2 text-sm text-foreground/60">{group.title}</h4>
            <div
              className={
                group.wide
                  ? "grid grid-cols-2 gap-x-6 gap-y-0.5"
                  : "space-y-0.5"
              }
            >
              {group.items.map((item) => (
                <div
                  key={item.label}
                  className="flex items-center justify-between py-0.5"
                >
                  <MetricLabel item={item} />
                  <span
                    className={cn(
                      "text-sm font-mono tabular-nums",
                      item.color ? VALUE_COLORS[item.color] : "",
                    )}
                  >
                    {item.value}
                  </span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
