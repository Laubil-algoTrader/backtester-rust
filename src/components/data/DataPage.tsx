import { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Upload,
  Download,
  Trash2,
  Eye,
  Database,
  Clock,
  RefreshCw,
  X,
  AlertCircle,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Progress } from "@/components/ui/Progress";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/Dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { DownloadDialog } from "@/components/data/DownloadDialog";
import { useAppStore } from "@/stores/useAppStore";
import {
  uploadCsv,
  getSymbols,
  deleteSymbol,
  previewData,
  cancelDownload,
  transformSymbolTimezone,
} from "@/lib/tauri";
import { INSTRUMENT_PRESETS, type Symbol, type InstrumentConfig, type TickStorageFormat } from "@/lib/types";
import { TIMEZONE_OPTIONS, formatTzOffset } from "@/lib/timezones";

// ── Import CSV dialog ──────────────────────────────────────────────────────────

interface ImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  filePath: string;
}

function ImportDialog({ open, onOpenChange, filePath }: ImportDialogProps) {
  const { t } = useTranslation("data");
  const { t: tc } = useTranslation("common");
  const addSymbol = useAppStore((s) => s.addSymbol);

  const [symbolName, setSymbolName] = useState("");
  const [preset, setPreset] = useState("Forex Major");
  const [config, setConfig] = useState<InstrumentConfig>(INSTRUMENT_PRESETS["Forex Major"]);
  const [storageFormat, setStorageFormat] = useState<TickStorageFormat>("Parquet");
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState(0);
  const [progressMsg, setProgressMsg] = useState("");
  const [error, setError] = useState("");

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      const base = filePath.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "";
      setSymbolName(base.toUpperCase());
      setPreset("Forex Major");
      setConfig(INSTRUMENT_PRESETS["Forex Major"]);
      setImporting(false);
      setProgress(0);
      setProgressMsg("");
      setError("");
    }
  }, [open, filePath]);

  // Listen to conversion-progress events while the dialog is open
  useEffect(() => {
    if (!open) return;
    const unlisten = listen<{ percent: number; message: string }>(
      "conversion-progress",
      (e) => {
        setProgress(e.payload.percent);
        setProgressMsg(e.payload.message);
      }
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [open]);

  const handlePresetChange = (value: string) => {
    setPreset(value);
    if (value in INSTRUMENT_PRESETS) {
      setConfig(INSTRUMENT_PRESETS[value]);
    }
  };

  const handleImport = async () => {
    if (!symbolName.trim() || !filePath) return;
    setError("");
    setImporting(true);
    try {
      const symbol = await uploadCsv(filePath, symbolName.trim(), config, storageFormat);
      addSymbol(symbol);
      onOpenChange(false);
      toast.success(`${symbolName} imported successfully`);
    } catch (err: unknown) {
      const msg =
        typeof err === "string" ? err
        : (err as { message?: string })?.message
        ?? JSON.stringify(err);
      setError(msg);
      setImporting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={importing ? undefined : onOpenChange}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>{t("import.title")}</DialogTitle>
          <DialogDescription>{t("import.desc")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* File path */}
          <div className="space-y-1">
            <label className="text-sm font-medium">{t("import.csvFile")}</label>
            <p className="truncate rounded-md bg-muted px-3 py-1.5 text-xs text-muted-foreground">
              {filePath || t("import.selectFile")}
            </p>
          </div>

          {/* Symbol name */}
          <div className="space-y-1">
            <label className="text-sm font-medium">{t("symbolName")}</label>
            <Input
              value={symbolName}
              onChange={(e) => setSymbolName(e.target.value.toUpperCase())}
              placeholder="e.g. EURUSD"
              disabled={importing}
            />
          </div>

          {/* Instrument preset */}
          <div className="space-y-1">
            <label className="text-sm font-medium">{t("import.instrumentType")}</label>
            <Select value={preset} onValueChange={handlePresetChange} disabled={importing}>
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.keys(INSTRUMENT_PRESETS).map((name) => (
                  <SelectItem key={name} value={name}>{name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Tick storage format — only relevant for tick CSV files */}
          <div className="space-y-1">
            <label className="text-sm font-medium">{t("downloadDialog.storageFormat")}</label>
            <div className="flex gap-2">
              {(["Parquet", "Binary"] as TickStorageFormat[]).map((fmt) => (
                <button
                  key={fmt}
                  onClick={() => setStorageFormat(fmt)}
                  disabled={importing}
                  className={`flex-1 rounded-md border px-3 py-1.5 text-xs font-medium transition-colors ${
                    storageFormat === fmt
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border bg-background text-muted-foreground hover:bg-muted/50"
                  }`}
                >
                  {t(`downloadDialog.storageFormat${fmt}`)}
                </button>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">
              {t(`downloadDialog.storageFormat${storageFormat}Desc`)}
              {" "}Only applies to tick data — OHLCV bars always use Parquet.
            </p>
          </div>

          {/* Key config fields */}
          <div className="grid grid-cols-3 gap-2">
            {[
              { key: "pip_size" as const, label: t("pipSize") },
              { key: "pip_value" as const, label: t("pipValue") },
              { key: "lot_size" as const, label: t("lotSize") },
            ].map(({ key, label }) => (
              <div key={key} className="space-y-1">
                <label className="text-xs text-muted-foreground">{label}</label>
                <Input
                  type="number"
                  step="any"
                  value={config[key]}
                  onChange={(e) => setConfig({ ...config, [key]: parseFloat(e.target.value) || 0 })}
                  className="h-8 text-xs"
                  disabled={importing}
                />
              </div>
            ))}
          </div>

          {/* Progress */}
          {importing && (
            <div className="space-y-1.5">
              <Progress value={progress} className="h-1.5" />
              <p className="text-xs text-muted-foreground">{progressMsg}</p>
            </div>
          )}

          {error && <p className="text-sm text-destructive">{t("import.importFailed")}: {error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={importing}>
            {tc("buttons.cancel")}
          </Button>
          <Button
            onClick={handleImport}
            disabled={importing || !symbolName.trim()}
          >
            {importing ? t("import.importing") : t("import.importBtn")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Preview dialog ─────────────────────────────────────────────────────────────

interface PreviewDialogProps {
  symbol: Symbol | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function PreviewDialog({ symbol, open, onOpenChange }: PreviewDialogProps) {
  const { t } = useTranslation("data");
  const { t: tc } = useTranslation("common");
  const [rows, setRows] = useState<Record<string, unknown>[]>([]);
  const [loading, setLoading] = useState(false);
  const [timeframe, setTimeframe] = useState("m1");

  const timeframes = symbol
    ? Object.keys(symbol.timeframe_paths).filter((tf) => !tf.includes("raw"))
    : [];

  useEffect(() => {
    if (!open || !symbol) return;
    const tf = timeframes.includes(timeframe) ? timeframe : (timeframes[0] ?? "m1");
    setTimeframe(tf);
    setLoading(true);
    previewData(symbol.id, tf, 50)
      .then(setRows)
      .catch((e) => toast.error(String(e)))
      .finally(() => setLoading(false));
  }, [open, symbol, timeframe]); // eslint-disable-line react-hooks/exhaustive-deps

  const columns = rows.length > 0 ? Object.keys(rows[0]) : [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[800px] max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t("preview")} — {symbol?.name}</DialogTitle>
        </DialogHeader>

        {/* Timeframe selector */}
        {timeframes.length > 1 && (
          <div className="flex gap-1 flex-wrap">
            {timeframes.map((tf) => (
              <button
                key={tf}
                onClick={() => setTimeframe(tf)}
                className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
                  timeframe === tf
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/80"
                }`}
              >
                {tf.toUpperCase()}
              </button>
            ))}
          </div>
        )}

        <div className="flex-1 overflow-auto">
          {loading ? (
            <p className="py-8 text-center text-sm text-muted-foreground">{t("loading")}</p>
          ) : rows.length === 0 ? (
            <p className="py-8 text-center text-sm text-muted-foreground">{t("noDataAvailable")}</p>
          ) : (
            <>
              <p className="mb-2 text-xs text-muted-foreground">
                {t("showingRows", { count: rows.length })}
              </p>
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b border-border">
                    {columns.map((col) => (
                      <th key={col} className="px-2 py-1 text-left font-medium text-muted-foreground">
                        {col}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {rows.map((row, i) => (
                    <tr key={i} className="border-b border-border/50 hover:bg-muted/30">
                      {columns.map((col) => (
                        <td key={col} className="px-2 py-1 font-mono">
                          {String(row[col] ?? "")}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {tc("buttons.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Timezone transform dialog ──────────────────────────────────────────────────

interface TzDialogProps {
  symbol: Symbol | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function TzDialog({ symbol, open, onOpenChange }: TzDialogProps) {
  const { t } = useTranslation("data");
  const { t: tc } = useTranslation("common");
  const updateSymbol = useAppStore((s) => s.updateSymbol);

  const [newTz, setNewTz] = useState(0);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (open && symbol) {
      setNewTz(symbol.instrument_config.tz_offset_hours ?? 0);
      setError("");
    }
  }, [open, symbol]);

  const handleApply = async () => {
    if (!symbol) return;
    setError("");
    setApplying(true);
    try {
      const updated = await transformSymbolTimezone(symbol.id, newTz);
      updateSymbol(updated);
      onOpenChange(false);
      toast.success(`Timezone updated for ${symbol.name}`);
    } catch (err) {
      setError(t("tzTransform.failed") + ": " + String(err));
    } finally {
      setApplying(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={applying ? undefined : onOpenChange}>
      <DialogContent className="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>{t("tzTransform.title")}</DialogTitle>
          <DialogDescription>
            {t("tzTransform.desc", { name: symbol?.name ?? "" })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1">
            <label className="text-sm text-muted-foreground">{t("tzTransform.current")}</label>
            <p className="text-sm font-medium">
              {formatTzOffset(symbol?.instrument_config.tz_offset_hours ?? 0)}
            </p>
          </div>

          <div className="space-y-1">
            <label className="text-sm font-medium">{t("tzTransform.newTz")}</label>
            <Select
              value={String(newTz)}
              onValueChange={(v) => setNewTz(parseFloat(v))}
              disabled={applying}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue>{formatTzOffset(newTz)}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                {TIMEZONE_OPTIONS.map((tz) => (
                  <SelectItem key={tz.value} value={String(tz.value)}>
                    {tz.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={applying}>
            {t("tzTransform.cancel")}
          </Button>
          <Button onClick={handleApply} disabled={applying}>
            {applying ? t("tzTransform.applying") : t("tzTransform.apply")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Symbol card ────────────────────────────────────────────────────────────────

interface SymbolCardProps {
  symbol: Symbol;
  isSelected: boolean;
  downloadProgress?: { progress: number; message: string };
  onSelect: () => void;
  onDelete: () => void;
  onPreview: () => void;
  onTransformTz: () => void;
  onCancelDownload: () => void;
  onResume: () => void;
}

function SymbolCard({
  symbol,
  isSelected,
  downloadProgress,
  onSelect,
  onDelete,
  onPreview,
  onTransformTz,
  onCancelDownload,
  onResume,
}: SymbolCardProps) {
  const { t } = useTranslation("data");
  const isDownloading = !!downloadProgress;
  const isInterrupted = symbol.status === "downloading" && !isDownloading;
  const timeframeCount = Object.keys(symbol.timeframe_paths).filter(
    (tf) => !tf.includes("raw")
  ).length;

  return (
    <div
      onClick={onSelect}
      className={`relative cursor-pointer rounded-lg border p-4 transition-colors hover:bg-muted/30 ${
        isSelected ? "border-primary bg-primary/5" : "border-border"
      }`}
    >
      {/* Header row */}
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-semibold">{symbol.name}</span>
            <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground uppercase">
              {symbol.base_timeframe}
            </span>
            {isInterrupted && (
              <span className="flex items-center gap-1 rounded bg-amber-500/15 px-1.5 py-0.5 text-xs text-amber-500">
                <AlertCircle className="h-3 w-3" />
                {t("interrupted")}
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {symbol.start_date?.slice(0, 10)} → {symbol.end_date?.slice(0, 10)}
          </p>
        </div>

        {/* Actions */}
        <div className="flex shrink-0 items-center gap-1" onClick={(e) => e.stopPropagation()}>
          {isInterrupted ? (
            <Button size="sm" variant="outline" className="h-7 text-xs" onClick={onResume}>
              <RefreshCw className="mr-1 h-3 w-3" />
              {t("resumeDownload")}
            </Button>
          ) : (
            <>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 w-7 p-0"
                title={t("tzTransform.button")}
                onClick={onTransformTz}
                disabled={isDownloading}
              >
                <Clock className="h-3.5 w-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 w-7 p-0"
                title={t("preview")}
                onClick={onPreview}
                disabled={isDownloading}
              >
                <Eye className="h-3.5 w-3.5" />
              </Button>
              {isDownloading ? (
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 w-7 p-0 text-destructive hover:text-destructive"
                  title={t("cancelDownload")}
                  onClick={onCancelDownload}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-7 w-7 p-0 text-destructive hover:text-destructive"
                  title={t("deleteConfirm", { name: symbol.name })}
                  onClick={onDelete}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              )}
            </>
          )}
        </div>
      </div>

      {/* Stats row */}
      {!isInterrupted && (
        <div className="mt-2 flex items-center gap-4 text-xs text-muted-foreground">
          <span className="flex items-center gap-1">
            <Database className="h-3 w-3" />
            {symbol.total_rows.toLocaleString()} {t("rows")}
          </span>
          <span>{timeframeCount} {t("timeframes")}</span>
          <span>{formatTzOffset(symbol.instrument_config.tz_offset_hours ?? 0)}</span>
        </div>
      )}

      {/* Download progress */}
      {isDownloading && (
        <div className="mt-3 space-y-1">
          <Progress value={downloadProgress.progress} className="h-1.5" />
          <p className="text-xs text-muted-foreground">{downloadProgress.message}</p>
        </div>
      )}
    </div>
  );
}

// ── Main DataPage ──────────────────────────────────────────────────────────────

export function DataPage() {
  const { t } = useTranslation("data");

  const symbols = useAppStore((s) => s.symbols);
  const selectedSymbolId = useAppStore((s) => s.selectedSymbolId);
  const setSymbols = useAppStore((s) => s.setSymbols);
  const removeSymbol = useAppStore((s) => s.removeSymbol);
  const setSelectedSymbolId = useAppStore((s) => s.setSelectedSymbolId);
  const activeDownloads = useAppStore((s) => s.activeDownloads);
  const updateDownloadProgress = useAppStore((s) => s.updateDownloadProgress);

  const [downloadDialogOpen, setDownloadDialogOpen] = useState(false);
  const [resumeSymbol, setResumeSymbol] = useState<Symbol | undefined>(undefined);
  const [importFilePath, setImportFilePath] = useState("");
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [previewSymbol, setPreviewSymbol] = useState<Symbol | null>(null);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [tzSymbol, setTzSymbol] = useState<Symbol | null>(null);
  const [tzOpen, setTzOpen] = useState(false);

  // Load symbols once on mount
  useEffect(() => {
    getSymbols()
      .then(setSymbols)
      .catch((e) => toast.error(String(e)));
  }, [setSymbols]);

  // Listen for download progress events
  useEffect(() => {
    const unlisten = listen<{ symbol_name: string; percent: number; message: string }>(
      "download-progress",
      (e) => {
        updateDownloadProgress(e.payload.symbol_name, e.payload.percent, e.payload.message);
      }
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [updateDownloadProgress]);

  const handleImportClick = useCallback(async () => {
    const selected = await openDialog({
      filters: [{ name: "CSV files", extensions: ["csv", "txt"] }],
      multiple: false,
    });
    if (!selected || typeof selected !== "string") return;
    setImportFilePath(selected);
    setImportDialogOpen(true);
  }, []);

  const handleDelete = useCallback(
    async (symbol: Symbol) => {
      try {
        await deleteSymbol(symbol.id);
        removeSymbol(symbol.id);
        toast.success(`${symbol.name} deleted`);
      } catch (err) {
        toast.error(String(err));
      }
    },
    [removeSymbol]
  );

  const handleCancelDownload = useCallback(async (symbolName: string) => {
    try {
      await cancelDownload(symbolName);
    } catch {
      // ignore
    }
  }, []);

  const handleResume = useCallback((symbol: Symbol) => {
    setResumeSymbol(symbol);
    setDownloadDialogOpen(true);
  }, []);

  const openPreview = useCallback((symbol: Symbol) => {
    setPreviewSymbol(symbol);
    setPreviewOpen(true);
  }, []);

  const openTzDialog = useCallback((symbol: Symbol) => {
    setTzSymbol(symbol);
    setTzOpen(true);
  }, []);

  const handleDownloadDialogClose = (open: boolean) => {
    setDownloadDialogOpen(open);
    if (!open) setResumeSymbol(undefined);
  };

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">{t("importedSymbols")}</h1>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={handleImportClick}>
            <Upload className="mr-2 h-4 w-4" />
            {t("importCsv")}
          </Button>
          <Button size="sm" onClick={() => setDownloadDialogOpen(true)}>
            <Download className="mr-2 h-4 w-4" />
            {t("download")}
          </Button>
        </div>
      </div>

      {/* Symbol list */}
      {symbols.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border p-12 text-center">
          <Database className="h-10 w-10 text-muted-foreground/40" />
          <div>
            <p className="font-medium text-muted-foreground">{t("noDataYet")}</p>
            <p className="mt-1 text-sm text-muted-foreground/70">{t("noDataDesc")}</p>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={handleImportClick}>
              <Upload className="mr-2 h-4 w-4" />
              {t("importCsv")}
            </Button>
            <Button size="sm" onClick={() => setDownloadDialogOpen(true)}>
              <Download className="mr-2 h-4 w-4" />
              {t("download")}
            </Button>
          </div>
        </div>
      ) : (
        <div className="flex-1 space-y-2 overflow-auto">
          {symbols.map((symbol) => (
            <SymbolCard
              key={symbol.id}
              symbol={symbol}
              isSelected={selectedSymbolId === symbol.id}
              downloadProgress={activeDownloads[symbol.name]}
              onSelect={() => setSelectedSymbolId(symbol.id)}
              onDelete={() => handleDelete(symbol)}
              onPreview={() => openPreview(symbol)}
              onTransformTz={() => openTzDialog(symbol)}
              onCancelDownload={() => handleCancelDownload(symbol.name)}
              onResume={() => handleResume(symbol)}
            />
          ))}
        </div>
      )}

      {/* Dialogs */}
      <ImportDialog
        open={importDialogOpen}
        onOpenChange={setImportDialogOpen}
        filePath={importFilePath}
      />

      <DownloadDialog
        open={downloadDialogOpen}
        onOpenChange={handleDownloadDialogClose}
        resumeSymbol={resumeSymbol}
      />

      <PreviewDialog
        symbol={previewSymbol}
        open={previewOpen}
        onOpenChange={setPreviewOpen}
      />

      <TzDialog
        symbol={tzSymbol}
        open={tzOpen}
        onOpenChange={setTzOpen}
      />
    </div>
  );
}
