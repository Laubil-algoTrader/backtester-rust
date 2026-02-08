import { useAppStore } from "@/stores/useAppStore";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/Tabs";
import { BarChart3 } from "lucide-react";
import { BacktestPanel } from "./BacktestPanel";
import { MetricsGrid } from "./MetricsGrid";
import { EquityCurve } from "./EquityCurve";
import { DrawdownChart } from "./DrawdownChart";
import { ReturnsHistogram } from "./ReturnsHistogram";
import { TradesList } from "./TradesList";

export function BacktestPage() {
  const { backtestResults } = useAppStore();

  return (
    <div className="space-y-4">
      <BacktestPanel />

      {!backtestResults && (
        <div className="py-12 text-center">
          <BarChart3 className="mx-auto mb-3 h-10 w-10 text-muted-foreground/40" />
          <p className="text-sm text-muted-foreground">
            Configure and run a backtest to see results here.
          </p>
        </div>
      )}

      {backtestResults && (
        <>
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Performance Metrics</CardTitle>
            </CardHeader>
            <CardContent>
              <MetricsGrid metrics={backtestResults.metrics} />
            </CardContent>
          </Card>

          <Card>
            <CardContent className="pt-6">
              <Tabs defaultValue="equity">
                <TabsList>
                  <TabsTrigger value="equity">Equity Curve</TabsTrigger>
                  <TabsTrigger value="drawdown">Drawdown</TabsTrigger>
                  <TabsTrigger value="returns">Returns</TabsTrigger>
                  <TabsTrigger value="trades">
                    Trades ({backtestResults.trades.length})
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="equity" className="pt-4">
                  <EquityCurve data={backtestResults.equity_curve} />
                </TabsContent>

                <TabsContent value="drawdown" className="pt-4">
                  <DrawdownChart data={backtestResults.drawdown_curve} />
                </TabsContent>

                <TabsContent value="returns" className="pt-4">
                  <ReturnsHistogram returns={backtestResults.returns} />
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
