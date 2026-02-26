import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { Lock, Zap, Code2 } from "lucide-react";

interface ProGateProps {
  feature: "optimization" | "export";
  children: React.ReactNode;
}

export function ProGate({ feature, children }: ProGateProps) {
  const { t } = useTranslation("auth");
  const licenseTier = useAppStore((s) => s.licenseTier);

  if (licenseTier === "pro") {
    return <>{children}</>;
  }

  const title = feature === "optimization" ? t("proGate.optimizationTitle") : t("proGate.exportTitle");
  const description = feature === "optimization"
    ? t("proGate.optimizationDesc")
    : t("proGate.exportDesc");
  const Icon = feature === "optimization" ? Zap : Code2;

  return (
    <div className="mx-auto flex max-w-md flex-col items-center justify-center py-24 text-center">
      <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-primary/10">
        <Lock className="h-6 w-6 text-primary" />
      </div>
      <h2 className="mb-2 text-xl font-bold text-foreground">
        {title}
      </h2>
      <p className="mb-4 text-sm text-muted-foreground">
        {description}
      </p>
      <div className="rounded-lg border border-primary/20 bg-primary/[0.03] p-4">
        <div className="flex items-center gap-2 text-sm text-primary">
          <Icon className="h-4 w-4" />
          <span className="font-medium">{t("proGate.proFeature")}</span>
        </div>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("proGate.upgradeMessage")}
        </p>
      </div>
    </div>
  );
}
