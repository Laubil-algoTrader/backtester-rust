import type { BacktestMetrics } from "@/lib/types";

interface MetricItem {
  label: string;
  value: string;
  color?: "green" | "red" | "neutral";
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

function buildGroups(m: BacktestMetrics): MetricGroup[] {
  return [
    {
      title: "Returns",
      items: [
        { label: "Final Capital", value: `$${fmt(m.final_capital)}`, color: colorBySign(m.total_return_pct) },
        { label: "Total Return", value: fmtPct(m.total_return_pct), color: colorBySign(m.total_return_pct) },
        { label: "Annualized", value: fmtPct(m.annualized_return_pct), color: colorBySign(m.annualized_return_pct) },
        { label: "Monthly Avg", value: fmtPct(m.monthly_return_avg_pct), color: colorBySign(m.monthly_return_avg_pct) },
      ],
    },
    {
      title: "Risk-Adjusted",
      items: [
        { label: "Sharpe", value: fmt(m.sharpe_ratio), color: colorBySign(m.sharpe_ratio) },
        { label: "Sortino", value: fmt(m.sortino_ratio), color: colorBySign(m.sortino_ratio) },
        { label: "Calmar", value: fmt(m.calmar_ratio), color: colorBySign(m.calmar_ratio) },
      ],
    },
    {
      title: "Drawdown",
      items: [
        { label: "Max DD", value: fmtPct(m.max_drawdown_pct), color: "red" },
        { label: "Max DD Duration", value: m.max_drawdown_duration_time },
        { label: "Avg DD", value: fmtPct(m.avg_drawdown_pct) },
        { label: "Recovery Factor", value: fmt(m.recovery_factor) },
      ],
    },
    {
      title: "Trades",
      items: [
        { label: "Total", value: String(m.total_trades) },
        { label: "Winners", value: String(m.winning_trades), color: "green" },
        { label: "Losers", value: String(m.losing_trades), color: "red" },
        { label: "Win Rate", value: fmtPct(m.win_rate_pct), color: m.win_rate_pct >= 50 ? "green" : "red" },
      ],
    },
    {
      title: "P&L",
      items: [
        { label: "Net Profit", value: `$${fmt(m.net_profit)}`, color: colorBySign(m.net_profit) },
        { label: "Profit Factor", value: fmt(m.profit_factor), color: m.profit_factor >= 1 ? "green" : "red" },
        { label: "Avg Trade", value: `$${fmt(m.avg_trade)}`, color: colorBySign(m.avg_trade) },
        { label: "Avg Win", value: `$${fmt(m.avg_win)}`, color: "green" },
        { label: "Avg Loss", value: `$${fmt(m.avg_loss)}`, color: "red" },
        { label: "Largest Win", value: `$${fmt(m.largest_win)}`, color: "green" },
        { label: "Largest Loss", value: `$${fmt(m.largest_loss)}`, color: "red" },
        { label: "Expectancy", value: `$${fmt(m.expectancy)}`, color: colorBySign(m.expectancy) },
      ],
    },
    {
      title: "Consistency",
      items: [
        { label: "Max Consec Wins", value: String(m.max_consecutive_wins) },
        { label: "Max Consec Losses", value: String(m.max_consecutive_losses) },
        { label: "Avg Consec Wins", value: fmt(m.avg_consecutive_wins, 1) },
        { label: "Avg Consec Losses", value: fmt(m.avg_consecutive_losses, 1) },
      ],
    },
    {
      title: "Risk",
      items: [
        { label: "MAE Avg", value: fmt(m.mae_avg, 1) },
        { label: "MAE Max", value: fmt(m.mae_max, 1) },
        { label: "MFE Avg", value: fmt(m.mfe_avg, 1) },
        { label: "MFE Max", value: fmt(m.mfe_max, 1) },
      ],
    },
  ];
}

const COLOR_MAP = {
  green: "text-emerald-500",
  red: "text-red-500",
  neutral: "",
};

interface MetricsGridProps {
  metrics: BacktestMetrics;
}

export function MetricsGrid({ metrics }: MetricsGridProps) {
  const groups = buildGroups(metrics);

  return (
    <div className="space-y-4">
      {groups.map((group) => (
        <div key={group.title}>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            {group.title}
          </h4>
          <div className="grid grid-cols-2 gap-2 md:grid-cols-3 lg:grid-cols-4">
            {group.items.map((item) => (
              <div
                key={item.label}
                className="rounded-md border bg-card p-2.5"
              >
                <p className="text-[11px] text-muted-foreground">
                  {item.label}
                </p>
                <p
                  className={`text-sm font-semibold ${
                    item.color ? COLOR_MAP[item.color] : ""
                  }`}
                >
                  {item.value}
                </p>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
