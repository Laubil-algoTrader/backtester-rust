import { Settings2 } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";

export function OptimizationPage() {
  return (
    <div className="space-y-4">
      <h2 className="text-xl font-semibold">Optimization</h2>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-muted-foreground">
            <Settings2 className="h-5 w-5" />
            Optimization will be implemented in Phase 5
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground">
            Optimize strategy parameters using Grid Search or Genetic Algorithm.
            Find the best parameter combinations across your historical data.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
