import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { validateLicense, loadSavedLicense } from "@/lib/tauri";
import { Input } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { Loader2, KeyRound, User, Zap, BarChart3, Code2, Settings2, ExternalLink } from "lucide-react";

const REGISTER_URL = "https://lb-quant.com/register";

export function LoginPage() {
  const { t } = useTranslation("auth");
  const { setLicenseInfo, setLicenseChecked } = useAppStore();

  const [username, setUsername] = useState("");
  const [licenseKey, setLicenseKey] = useState("");
  const [remember, setRemember] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);

  // On mount, try to load saved credentials
  useEffect(() => {
    (async () => {
      try {
        const saved = await loadSavedLicense();
        if (saved) {
          setUsername(saved.username);
          setLicenseKey(saved.license_key);
          setRemember(true);
          // Auto-validate saved credentials
          const response = await validateLicense(saved.username, saved.license_key, false);
          if (response.valid) {
            setLicenseInfo(response.tier, saved.username);
            setLicenseChecked(true);
            return;
          }
        }
      } catch {
        // Ignore errors loading saved license
      }
      setInitialLoading(false);
    })();
  }, [setLicenseInfo, setLicenseChecked]);

  const handleActivate = async () => {
    if (!username.trim()) {
      setError(t("usernameRequired"));
      return;
    }
    if (!licenseKey.trim()) {
      setError(t("licenseRequired"));
      return;
    }
    setError(null);
    setLoading(true);
    try {
      const response = await validateLicense(username.trim(), licenseKey.trim(), remember);
      if (response.valid) {
        setLicenseInfo(response.tier, username.trim());
        setLicenseChecked(true);
      } else {
        setError(response.message ?? t("invalidLicense"));
      }
    } catch (err) {
      setError(typeof err === "string" ? err : String(err));
    } finally {
      setLoading(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !loading) {
      handleActivate();
    }
  };

  if (initialLoading) {
    return (
      <div className="flex h-screen items-center justify-center bg-background">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-[880px]">
        {/* Header */}
        <div className="mb-8 text-center">
          <h1 className="text-3xl font-bold tracking-tight text-foreground">
            {t("title")}
          </h1>
          <p className="mt-1.5 text-sm text-muted-foreground">
            {t("subtitle")}
          </p>
        </div>

        <div className="flex gap-6">
          {/* Left: Login form */}
          <div className="flex-1 rounded-lg border border-border/40 bg-card p-6">
            <h2 className="mb-4 text-lg font-semibold text-foreground">
              {t("signIn")}
            </h2>

            <div className="space-y-3" onKeyDown={handleKeyDown}>
              <div>
                <label className="mb-1 block text-sm text-muted-foreground">
                  {t("username")}
                </label>
                <div className="relative">
                  <User className="absolute left-2.5 top-2 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    placeholder={t("usernamePlaceholder")}
                    className="pl-8"
                    autoFocus
                  />
                </div>
              </div>

              <div>
                <label className="mb-1 block text-sm text-muted-foreground">
                  {t("licenseKey")}
                </label>
                <div className="relative">
                  <KeyRound className="absolute left-2.5 top-2 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={licenseKey}
                    onChange={(e) => setLicenseKey(e.target.value)}
                    placeholder={t("licenseKeyPlaceholder")}
                    className="pl-8 font-mono"
                  />
                </div>
              </div>

              <label className="flex items-center gap-2 text-sm text-muted-foreground">
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(e) => setRemember(e.target.checked)}
                  className="h-3.5 w-3.5 accent-primary"
                />
                {t("rememberMe")}
              </label>

              {error && (
                <div className="rounded border border-destructive/50 bg-destructive/10 px-3 py-2">
                  <p className="text-sm text-destructive">{error}</p>
                </div>
              )}

              <Button
                onClick={handleActivate}
                disabled={loading || !username.trim() || !licenseKey.trim()}
                className="w-full"
              >
                {loading ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <KeyRound className="mr-2 h-4 w-4" />
                )}
                {t("signIn")}
              </Button>

              <div className="pt-1 text-center">
                <p className="text-xs text-muted-foreground">
                  {t("newHere")}{" "}
                  <a
                    href={REGISTER_URL}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-primary hover:underline"
                  >
                    {t("createFreeAccount")}
                    <ExternalLink className="h-3 w-3" />
                  </a>
                </p>
              </div>
            </div>
          </div>

          {/* Right: Tier comparison */}
          <div className="flex-1 space-y-3">
            {/* Free tier */}
            <div className="rounded-lg border border-border/40 bg-card p-4">
              <h3 className="mb-2 text-sm font-semibold text-foreground">
                {t("freeTier.title")}
              </h3>
              <ul className="space-y-1.5 text-sm text-muted-foreground">
                <li className="flex items-center gap-2">
                  <BarChart3 className="h-3.5 w-3.5 text-emerald-500" />
                  {t("freeTier.importData")}
                </li>
                <li className="flex items-center gap-2">
                  <Settings2 className="h-3.5 w-3.5 text-emerald-500" />
                  {t("freeTier.strategyBuilder")}
                </li>
                <li className="flex items-center gap-2">
                  <Zap className="h-3.5 w-3.5 text-emerald-500" />
                  {t("freeTier.backtesting")}
                </li>
              </ul>
            </div>

            {/* Pro tier */}
            <div className="rounded-lg border border-primary/30 bg-primary/[0.03] p-4">
              <h3 className="mb-2 text-sm font-semibold text-primary">
                {t("proTier.title")}
              </h3>
              <ul className="space-y-1.5 text-sm text-muted-foreground">
                <li className="flex items-center gap-2">
                  <BarChart3 className="h-3.5 w-3.5 text-emerald-500" />
                  {t("proTier.everythingFree")}
                </li>
                <li className="flex items-center gap-2">
                  <Zap className="h-3.5 w-3.5 text-primary" />
                  {t("proTier.optimization")}
                </li>
                <li className="flex items-center gap-2">
                  <Code2 className="h-3.5 w-3.5 text-primary" />
                  {t("proTier.codeExport")}
                </li>
              </ul>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
