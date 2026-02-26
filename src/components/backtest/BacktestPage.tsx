import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { exportTradesCsv, exportReportHtml } from "@/lib/tauri";
import { save } from "@tauri-apps/plugin-dialog";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { BarChart3, Download, FileSpreadsheet } from "lucide-react";
import { BacktestPanel } from "./BacktestPanel";
import { MetricsGrid } from "./MetricsGrid";
import { EquityCurve } from "./EquityCurve";
import { DrawdownChart } from "./DrawdownChart";
import { MonthlyReturns } from "./MonthlyReturns";
import { TradesList } from "./TradesList";

export function BacktestPage() {
  const { t } = useTranslation("backtest");
  const { backtestResults, initialCapital, equityMarkers } = useAppStore();

  const handleExportTrades = async () => {
    if (!backtestResults) return;
    const path = await save({
      defaultPath: "trades.csv",
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (path) {
      await exportTradesCsv(backtestResults.trades, path);
    }
  };

  const handleExportReport = async () => {
    if (!backtestResults) return;
    const path = await save({
      defaultPath: "backtest_report.html",
      filters: [{ name: "HTML", extensions: ["html"] }],
    });
    if (path) {
      await exportReportHtml(backtestResults, path);
    }
  };

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <BacktestPanel />

      {!backtestResults && (
        <div className="py-12 text-center">
          <BarChart3 className="mx-auto mb-3 h-10 w-10 text-muted-foreground/30" />
          <p className="text-sm text-muted-foreground">
            {t("noResults")}
          </p>
        </div>
      )}

      {backtestResults && (
        <>
          <Card>
            <CardHeader className="flex flex-row items-center justify-between pb-3">
              <CardTitle>{t("performanceMetrics")}</CardTitle>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 text-sm"
                  onClick={handleExportTrades}
                >
                  <Download className="mr-1.5 h-3.5 w-3.5" />
                  {t("exportTrades")}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 text-sm"
                  onClick={handleExportReport}
                >
                  <FileSpreadsheet className="mr-1.5 h-3.5 w-3.5" />
                  {t("exportReport")}
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <MetricsGrid metrics={backtestResults.metrics} />
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle>{t("equityCurve")}</CardTitle>
            </CardHeader>
            <CardContent className="pb-2">
              <EquityCurve data={backtestResults.equity_curve} initialCapital={initialCapital} markers={equityMarkers} />
            </CardContent>
            <CardHeader className="pb-2 pt-0">
              <CardTitle>{t("drawdown")}</CardTitle>
            </CardHeader>
            <CardContent>
              <DrawdownChart data={backtestResults.drawdown_curve} />
            </CardContent>
          </Card>

          <Card>
            <CardContent className="pt-6">
              <Tabs defaultValue="monthly">
                <TabsList>
                  <TabsTrigger value="monthly">
                    {t("monthlyPerformance")}
                  </TabsTrigger>
                  <TabsTrigger value="trades">
                    {t("trades")} ({backtestResults.trades.length})
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="monthly" className="pt-4">
                  <MonthlyReturns equityCurve={backtestResults.equity_curve} />
                </TabsContent>

                <TabsContent value="trades" className="pt-4">
                  <TradesList trades={backtestResults.trades} />
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>
        </>
      )}
    </div>
  );
}
