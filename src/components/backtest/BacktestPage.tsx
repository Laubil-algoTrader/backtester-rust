import { LineChart } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";

export function BacktestPage() {
  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold">Backtest</h2>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-muted-foreground">
            <LineChart className="h-5 w-5" />
            Backtest runner will be implemented in Phase 4
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            Run backtests on your strategies and view detailed results including
            equity curves, drawdown charts, and comprehensive performance metrics.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
