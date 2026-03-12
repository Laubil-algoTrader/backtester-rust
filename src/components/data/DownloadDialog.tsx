import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Download, Search, X } from "lucide-react";
import { TIMEZONE_OPTIONS, formatTzOffset } from "@/lib/timezones";

import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { DatePicker } from "@/components/ui/DatePicker";
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
import { downloadDukascopy } from "@/lib/tauri";
import { useAppStore } from "@/stores/useAppStore";
import { INSTRUMENT_PRESETS, DUKASCOPY_INSTRUMENTS } from "@/lib/types";
import type {
  InstrumentConfig,
  DukascopyCategory,
  DukascopyInstrument,
  TickStorageFormat,
  TickPipeline,
} from "@/lib/types";

interface DownloadDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const CATEGORIES: (DukascopyCategory | "All")[] = [
  "All",
  "Forex",
  "Indices",
  "Commodities",
  "Crypto",
];

export function DownloadDialog({ open: isOpen, onOpenChange }: DownloadDialogProps) {
  const { t } = useTranslation("data");
  const { t: tc } = useTranslation("common");
  const addSymbol = useAppStore((s) => s.addSymbol);
  const addActiveDownload = useAppStore((s) => s.addActiveDownload);
  const removeActiveDownload = useAppStore((s) => s.removeActiveDownload);
  const activeDownloads = useAppStore((s) => s.activeDownloads);

  // Instrument selection
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState<DukascopyCategory | "All">("All");
  const [selectedInstrument, setSelectedInstrument] = useState<DukascopyInstrument | null>(null);

  // Custom symbol mode
  const [customSymbol, setCustomSymbol] = useState("");
  const [customPointValue, setCustomPointValue] = useState(100000);

  // Editable symbol name (storage name in the app — defaults to effectiveSymbol)
  const [symbolName, setSymbolName] = useState("");

  // Modeling / base timeframe
  type DownloadModeling = "tick" | "m1";
  const [modeling, setModeling] = useState<DownloadModeling>("m1");

  // Storage format (only applies to tick mode)
  const [storageFormat, setStorageFormat] = useState<TickStorageFormat>("Parquet");

  // Pipeline (only for tick mode)
  const [pipeline, setPipeline] = useState<TickPipeline>("direct");
  const [keepCsv, setKeepCsv] = useState(false);

  // Date range
  const [startDate, setStartDate] = useState("2024-01-01");
  const [endDate, setEndDate] = useState("2024-12-31");

  // Instrument config
  const [preset, setPreset] = useState("Forex Major");
  const [config, setConfig] = useState<InstrumentConfig>(
    INSTRUMENT_PRESETS["Forex Major"]
  );

  const [error, setError] = useState("");

  // Filter instruments
  const filtered = useMemo(() => {
    let list = DUKASCOPY_INSTRUMENTS;
    if (category !== "All") {
      list = list.filter((i) => i.category === category);
    }
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter(
        (i) =>
          i.name.toLowerCase().includes(q) ||
          i.symbol.toLowerCase().includes(q)
      );
    }
    return list;
  }, [category, search]);

  const handleSelectInstrument = (inst: DukascopyInstrument) => {
    setSelectedInstrument(inst);
    setCustomSymbol("");
    setSymbolName(inst.symbol);
    setPreset(inst.preset);
    if (inst.preset in INSTRUMENT_PRESETS) {
      setConfig(INSTRUMENT_PRESETS[inst.preset]);
    }
  };

  const handlePresetChange = (value: string) => {
    setPreset(value);
    if (value in INSTRUMENT_PRESETS) {
      setConfig(INSTRUMENT_PRESETS[value]);
    }
  };

  // Determine effective symbol/point_value.
  // api_symbol is the Dukascopy data-feed URL symbol (e.g. "BRENTCMDUSD").
  // Falls back to symbol (e.g. "EURUSD") when api_symbol is not set.
  const effectiveSymbol =
    selectedInstrument?.api_symbol ?? selectedInstrument?.symbol ?? customSymbol;
  const effectivePointValue = selectedInstrument?.point_value ?? customPointValue;

  // Check if this symbol is already downloading
  const isAlreadyDownloading = symbolName.trim().length > 0 && symbolName in activeDownloads;

  const canDownload =
    effectiveSymbol.trim().length > 0 &&
    symbolName.trim().length > 0 &&
    startDate &&
    endDate &&
    startDate < endDate &&
    !isAlreadyDownloading;

  const handleDownload = () => {
    if (!canDownload) return;

    setError("");

    // Add to active downloads in store
    addActiveDownload(symbolName);

    // Close dialog immediately
    onOpenChange(false);

    // Fire and forget — start download async
    downloadDukascopy(
      symbolName,
      effectiveSymbol,
      effectivePointValue,
      startDate,
      endDate,
      modeling,
      config,
      modeling === "tick" ? storageFormat : undefined,
      modeling === "tick" ? pipeline : undefined,
      modeling === "tick" && pipeline === "via_csv" ? keepCsv : undefined
    )
      .then((symbol) => {
        addSymbol(symbol);
        removeActiveDownload(symbolName);
      })
      .catch((e: unknown) => {
        const msg = typeof e === "string" ? e : String(e);
        if (!msg.includes("Cancelled")) {
          console.error(`Download failed for ${symbolName}:`, msg);
        }
        removeActiveDownload(symbolName);
      });
  };

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[600px] max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("downloadDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("downloadDialog.desc")}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Search & category filter */}
          <div className="space-y-2">
            <label className="text-sm font-medium">{t("downloadDialog.instrument")}</label>
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("downloadDialog.searchInstruments")}
                className="pl-9"
              />
            </div>

            {/* Category tabs */}
            <div className="flex gap-1">
              {CATEGORIES.map((cat) => (
                <button
                  key={cat}
                  onClick={() => setCategory(cat)}
                  className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
                    category === cat
                      ? "bg-primary text-primary-foreground"
                      : "bg-muted text-muted-foreground hover:bg-muted/80"
                  }`}
                >
                  {cat}
                </button>
              ))}
            </div>

            {/* Instrument list */}
            <div className="max-h-36 overflow-y-auto rounded-md border border-border bg-background">
              {filtered.length === 0 ? (
                <p className="p-3 text-center text-xs text-muted-foreground">
                  {t("downloadDialog.noInstruments")}
                </p>
              ) : (
                filtered.map((inst) => (
                  <button
                    key={inst.symbol}
                    onClick={() => handleSelectInstrument(inst)}
                    className={`flex w-full items-center justify-between px-3 py-1.5 text-left text-xs transition-colors hover:bg-muted/50 ${
                      selectedInstrument?.symbol === inst.symbol
                        ? "bg-primary/10 text-primary"
                        : ""
                    }`}
                  >
                    <span className="font-medium">{inst.name}</span>
                    <span className="text-muted-foreground">{inst.symbol}</span>
                  </button>
                ))
              )}
            </div>

            {/* Selected indicator */}
            {selectedInstrument && (
              <div className="flex items-center gap-2 rounded-md bg-primary/10 px-3 py-1.5 text-xs">
                <span className="font-medium text-primary">
                  {selectedInstrument.name} ({selectedInstrument.symbol})
                </span>
                <button
                  onClick={() => { setSelectedInstrument(null); setSymbolName(""); }}
                  className="ml-auto text-muted-foreground hover:text-foreground"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            )}
          </div>

          {/* Custom symbol */}
          {!selectedInstrument && (
            <div className="space-y-2">
              <label className="text-xs text-muted-foreground">
                {t("downloadDialog.customSymbolHint")}
              </label>
              <div className="grid grid-cols-2 gap-2">
                <div className="space-y-1">
                  <label className="text-xs text-muted-foreground">{t("downloadDialog.symbol")}</label>
                  <Input
                    value={customSymbol}
                    onChange={(e) => {
                      const upper = e.target.value.toUpperCase();
                      setCustomSymbol(upper);
                      setSymbolName(upper);
                    }}
                    placeholder="e.g. EURUSD"
                    className="h-8 text-xs"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-xs text-muted-foreground">{t("downloadDialog.pointValue")}</label>
                  <Input
                    type="number"
                    value={customPointValue}
                    onChange={(e) => setCustomPointValue(Number(e.target.value) || 1)}
                    className="h-8 text-xs"
                  />
                </div>
              </div>
            </div>
          )}

          {/* Symbol name */}
          {effectiveSymbol.trim().length > 0 && (
            <div className="space-y-1">
              <label className="text-sm font-medium">{t("symbolName")}</label>
              <Input
                value={symbolName}
                onChange={(e) => setSymbolName(e.target.value.toUpperCase())}
                placeholder={effectiveSymbol}
                className="h-8 text-xs"
              />
              <p className="text-xs text-muted-foreground">
                {t("downloadDialog.symbolNameHint")}
              </p>
            </div>
          )}

          {/* Modeling mode */}
          <div className="space-y-2">
            <label className="text-sm font-medium">{t("downloadDialog.dataModeling")}</label>
            <div className="flex gap-2">
              {(["tick", "m1"] as const).map((mode) => (
                <button
                  key={mode}
                  onClick={() => setModeling(mode)}
                  className={`flex-1 rounded-md border px-3 py-1.5 text-xs font-medium transition-colors ${
                    modeling === mode
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border bg-background text-muted-foreground hover:bg-muted/50"
                  }`}
                >
                  {mode === "tick" ? t("downloadDialog.tickData") : t("downloadDialog.m1Bars")}
                </button>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">
              {modeling === "tick"
                ? t("downloadDialog.tickDesc")
                : t("downloadDialog.m1Desc")}
            </p>
          </div>

          {/* Storage format — only for tick mode */}
          {modeling === "tick" && (
            <div className="space-y-2">
              <label className="text-sm font-medium">{t("downloadDialog.storageFormat")}</label>
              <div className="flex gap-2">
                {(["Parquet", "Binary"] as TickStorageFormat[]).map((fmt) => (
                  <button
                    key={fmt}
                    onClick={() => setStorageFormat(fmt)}
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
              </p>
            </div>
          )}

          {/* Pipeline — tick mode only */}
          {modeling === "tick" && (
            <div className="space-y-2">
              <label className="text-sm font-medium">{t("downloadDialog.pipeline")}</label>
              <div className="flex gap-2">
                {(["direct", "via_csv"] as TickPipeline[]).map((p) => (
                  <button
                    key={p}
                    onClick={() => setPipeline(p)}
                    className={`flex-1 rounded-md border px-3 py-1.5 text-xs font-medium transition-colors ${
                      pipeline === p
                        ? "border-primary bg-primary/10 text-primary"
                        : "border-border bg-background text-muted-foreground hover:bg-muted/50"
                    }`}
                  >
                    {p === "direct"
                      ? t("downloadDialog.pipelineDirect")
                      : t("downloadDialog.pipelineViaCsv")}
                  </button>
                ))}
              </div>
              <p className="text-xs text-muted-foreground">
                {pipeline === "direct"
                  ? t("downloadDialog.pipelineDirectDesc")
                  : t("downloadDialog.pipelineViaCsvDesc")}
              </p>

              {/* Keep CSV checkbox — only when Via CSV selected */}
              {pipeline === "via_csv" && (
                <label className="flex cursor-pointer items-center gap-2 text-xs text-muted-foreground">
                  <input
                    type="checkbox"
                    checked={keepCsv}
                    onChange={(e) => setKeepCsv(e.target.checked)}
                    className="h-3.5 w-3.5 rounded border-border accent-primary"
                  />
                  {t("downloadDialog.keepCsv")}
                </label>
              )}
            </div>
          )}

          {/* Date range */}
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1">
              <label className="text-sm font-medium">{t("downloadDialog.startDate")}</label>
              <DatePicker
                value={startDate}
                onChange={setStartDate}
              />
            </div>
            <div className="space-y-1">
              <label className="text-sm font-medium">{t("downloadDialog.endDate")}</label>
              <DatePicker
                value={endDate}
                onChange={setEndDate}
              />
            </div>
          </div>

          {/* Instrument config */}
          <div className="space-y-2">
            <label className="text-sm font-medium">{t("downloadDialog.instrumentType")}</label>
            <Select
              value={preset}
              onValueChange={handlePresetChange}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.keys(INSTRUMENT_PRESETS).map((name) => (
                  <SelectItem key={name} value={name}>
                    {name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid grid-cols-3 gap-2">
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("pipSize")}</label>
              <Input
                type="number"
                step="any"
                value={config.pip_size}
                onChange={(e) =>
                  setConfig({ ...config, pip_size: parseFloat(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("pipValue")}</label>
              <Input
                type="number"
                step="any"
                value={config.pip_value}
                onChange={(e) =>
                  setConfig({ ...config, pip_value: parseFloat(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("lotSize")}</label>
              <Input
                type="number"
                step="any"
                value={config.lot_size}
                onChange={(e) =>
                  setConfig({ ...config, lot_size: parseFloat(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("minLot")}</label>
              <Input
                type="number"
                step="any"
                value={config.min_lot}
                onChange={(e) =>
                  setConfig({ ...config, min_lot: parseFloat(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("tickSize")}</label>
              <Input
                type="number"
                step="any"
                value={config.tick_size}
                onChange={(e) =>
                  setConfig({ ...config, tick_size: parseFloat(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
            <div className="space-y-1">
              <label className="text-xs text-muted-foreground">{t("digits")}</label>
              <Input
                type="number"
                value={config.digits}
                onChange={(e) =>
                  setConfig({ ...config, digits: parseInt(e.target.value) || 0 })
                }
                className="h-8 text-xs"
              />
            </div>
          </div>

          {/* Timezone */}
          <div className="space-y-1">
            <label className="text-sm font-medium">{t("tzOffset")}</label>
            <Select
              value={String(config.tz_offset_hours ?? 0)}
              onValueChange={(v) =>
                setConfig({ ...config, tz_offset_hours: parseFloat(v) })
              }
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue>
                  {formatTzOffset(config.tz_offset_hours ?? 0)}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {TIMEZONE_OPTIONS.map((tz) => (
                  <SelectItem key={tz.value} value={String(tz.value)}>
                    {tz.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t("tzOffsetHint")}</p>
          </div>

          {/* Already downloading warning */}
          {isAlreadyDownloading && (
            <p className="text-xs text-amber-500">
              {t("downloadDialog.alreadyDownloading")}
            </p>
          )}

          {/* Error */}
          {error && (
            <p className="text-sm text-destructive">{error}</p>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
          >
            {tc("buttons.close")}
          </Button>
          <Button onClick={handleDownload} disabled={!canDownload}>
            <Download className="mr-2 h-4 w-4" />
            {t("download")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
