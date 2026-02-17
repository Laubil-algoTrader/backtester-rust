import { useState, useEffect, useCallback } from "react";
import { Copy, Check, Download, Code2, RefreshCw, FileCode2, FolderDown } from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile, mkdir } from "@tauri-apps/plugin-fs";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { useAppStore } from "@/stores/useAppStore";
import { generateStrategyCode } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { Strategy, CodeFile, CodeGenerationResult } from "@/lib/types";

type Language = "mql5" | "pinescript";

export function ExportPage() {
  const currentStrategy = useAppStore((s) => s.currentStrategy);
  const [language, setLanguage] = useState<Language>("mql5");
  const [result, setResult] = useState<CodeGenerationResult | null>(null);
  const [selectedFileIdx, setSelectedFileIdx] = useState(0);
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const hasRules =
    currentStrategy.long_entry_rules.length > 0 ||
    currentStrategy.short_entry_rules.length > 0;

  const generate = useCallback(async () => {
    if (!hasRules) {
      setResult(null);
      setError("No entry rules defined. Create a strategy first.");
      return;
    }
    setIsGenerating(true);
    setError(null);
    try {
      const strategyPayload: Strategy = {
        ...currentStrategy,
        id: currentStrategy.id ?? "",
        created_at: currentStrategy.created_at ?? "",
        updated_at: currentStrategy.updated_at ?? "",
      };
      const res = await generateStrategyCode(language, strategyPayload);
      setResult(res);
      // Select the main file by default
      const mainIdx = res.files.findIndex((f) => f.is_main);
      setSelectedFileIdx(mainIdx >= 0 ? mainIdx : 0);
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
      setResult(null);
    } finally {
      setIsGenerating(false);
    }
  }, [language, currentStrategy, hasRules]);

  useEffect(() => {
    generate();
  }, [generate]);

  const selectedFile: CodeFile | null =
    result && result.files[selectedFileIdx]
      ? result.files[selectedFileIdx]
      : null;

  const handleCopy = async () => {
    if (!selectedFile) return;
    await navigator.clipboard.writeText(selectedFile.code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownloadCurrent = async () => {
    if (!selectedFile) return;
    const path = await save({
      defaultPath: selectedFile.filename,
      filters: [
        {
          name: selectedFile.filename.endsWith(".mq5")
            ? "MQL5 File"
            : "Pine Script",
          extensions: [selectedFile.filename.split(".").pop() || "txt"],
        },
      ],
    });
    if (path) {
      await writeTextFile(path, selectedFile.code);
    }
  };

  const handleDownloadAll = async () => {
    if (!result || result.files.length === 0) return;

    if (result.files.length === 1) {
      // Single file — just save directly
      await handleDownloadCurrent();
      return;
    }

    // Multiple files — ask for a folder (use save dialog with folder name)
    const name = currentStrategy.name || "strategy";
    const safeName = name.replace(/[^a-zA-Z0-9_-]/g, "_");
    const folderPath = await save({
      defaultPath: safeName,
      filters: [{ name: "Folder", extensions: [""] }],
    });

    if (folderPath) {
      // Create folder and write all files
      try {
        await mkdir(folderPath, { recursive: true });
      } catch {
        // Folder may already exist
      }
      for (const file of result.files) {
        const filePath = `${folderPath}/${file.filename}`;
        await writeTextFile(filePath, file.code);
      }
    }
  };

  const lineCount = selectedFile ? selectedFile.code.split("\n").length : 0;
  const totalFiles = result ? result.files.length : 0;
  const indicatorFiles = result
    ? result.files.filter((f) => !f.is_main)
    : [];
  const mainFile = result ? result.files.find((f) => f.is_main) : null;

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Code2 className="h-4 w-4 text-primary" />
          <h2 className="text-xs font-semibold uppercase tracking-widest">
            Export Strategy Code
          </h2>
        </div>
        <div className="text-[10px] text-muted-foreground">
          {currentStrategy.name || "Untitled Strategy"}
        </div>
      </div>

      {/* Language selector */}
      <div className="flex items-center gap-2">
        <button
          onClick={() => setLanguage("mql5")}
          className={cn(
            "rounded px-3 py-1.5 text-[10px] font-medium uppercase tracking-wider transition-colors",
            language === "mql5"
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground hover:text-foreground"
          )}
        >
          MQL5 (MetaTrader 5)
        </button>
        <button
          onClick={() => setLanguage("pinescript")}
          className={cn(
            "rounded px-3 py-1.5 text-[10px] font-medium uppercase tracking-wider transition-colors",
            language === "pinescript"
              ? "bg-primary text-primary-foreground"
              : "bg-muted text-muted-foreground hover:text-foreground"
          )}
        >
          Pine Script v6 (TradingView)
        </button>

        <div className="ml-auto flex items-center gap-2">
          <button
            onClick={generate}
            disabled={isGenerating || !hasRules}
            className="flex items-center gap-1 rounded px-2 py-1.5 text-[10px] text-muted-foreground transition-colors hover:text-foreground disabled:opacity-40"
          >
            <RefreshCw
              className={cn("h-3 w-3", isGenerating && "animate-spin")}
            />
            Regenerate
          </button>
          <button
            onClick={handleCopy}
            disabled={!selectedFile}
            className="flex items-center gap-1 rounded bg-muted px-2.5 py-1.5 text-[10px] font-medium transition-colors hover:bg-muted/80 disabled:opacity-40"
          >
            {copied ? (
              <Check className="h-3 w-3 text-green-500" />
            ) : (
              <Copy className="h-3 w-3" />
            )}
            {copied ? "Copied" : "Copy"}
          </button>
          {totalFiles > 1 ? (
            <button
              onClick={handleDownloadAll}
              disabled={!result}
              className="flex items-center gap-1 rounded bg-primary px-2.5 py-1.5 text-[10px] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-40"
            >
              <FolderDown className="h-3 w-3" />
              Download All ({totalFiles} files)
            </button>
          ) : (
            <button
              onClick={handleDownloadCurrent}
              disabled={!selectedFile}
              className="flex items-center gap-1 rounded bg-primary px-2.5 py-1.5 text-[10px] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-40"
            >
              <Download className="h-3 w-3" />
              Download .{language === "mql5" ? "mq5" : "pine"}
            </button>
          )}
        </div>
      </div>

      {/* File tabs (when multiple files) */}
      {result && result.files.length > 1 && (
        <div className="flex flex-wrap gap-1">
          {/* Main file tab */}
          {mainFile && (
            <button
              onClick={() =>
                setSelectedFileIdx(result.files.indexOf(mainFile))
              }
              className={cn(
                "flex items-center gap-1.5 rounded px-2.5 py-1 text-[10px] font-medium transition-colors",
                selectedFile === mainFile
                  ? "bg-primary/15 text-primary ring-1 ring-primary/30"
                  : "bg-muted text-muted-foreground hover:text-foreground"
              )}
            >
              <Code2 className="h-3 w-3" />
              {mainFile.filename}
              <span className="rounded bg-primary/10 px-1 text-[8px] text-primary">
                EA
              </span>
            </button>
          )}
          {/* Indicator file tabs */}
          {indicatorFiles.map((file) => {
            const idx = result.files.indexOf(file);
            return (
              <button
                key={file.filename}
                onClick={() => setSelectedFileIdx(idx)}
                className={cn(
                  "flex items-center gap-1.5 rounded px-2.5 py-1 text-[10px] font-medium transition-colors",
                  selectedFileIdx === idx
                    ? "bg-primary/15 text-primary ring-1 ring-primary/30"
                    : "bg-muted text-muted-foreground hover:text-foreground"
                )}
              >
                <FileCode2 className="h-3 w-3" />
                {file.filename}
              </button>
            );
          })}
        </div>
      )}

      {/* Code preview */}
      <Card className="flex-1 overflow-hidden">
        <CardHeader className="py-3">
          <div className="flex items-center justify-between">
            <CardTitle>
              {selectedFile
                ? selectedFile.is_main
                  ? language === "mql5"
                    ? "Expert Advisor"
                    : "Strategy Script"
                  : `Custom Indicator — ${selectedFile.filename.replace(".mq5", "")}`
                : language === "mql5"
                  ? "Expert Advisor"
                  : "Strategy Script"}
            </CardTitle>
            {selectedFile && (
              <span className="text-[10px] text-muted-foreground">
                {lineCount} lines
              </span>
            )}
          </div>
        </CardHeader>
        <CardContent className="h-[calc(100%-60px)] overflow-hidden pb-4">
          {isGenerating ? (
            <div className="flex h-64 items-center justify-center">
              <RefreshCw className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <div className="flex h-64 flex-col items-center justify-center gap-2 text-muted-foreground">
              <Code2 className="h-8 w-8 opacity-40" />
              <p className="text-xs">{error}</p>
            </div>
          ) : selectedFile ? (
            <pre className="h-full overflow-auto rounded-md border border-border/40 bg-zinc-950 p-4 text-[11px] leading-relaxed text-zinc-300">
              <code>{selectedFile.code}</code>
            </pre>
          ) : (
            <div className="flex h-64 flex-col items-center justify-center gap-2 text-muted-foreground">
              <Code2 className="h-8 w-8 opacity-40" />
              <p className="text-xs">
                No code generated yet. Create a strategy with entry rules first.
              </p>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Platform notes */}
      <Card>
        <CardContent className="py-3">
          {language === "mql5" ? (
            <div className="space-y-1 text-[10px] text-muted-foreground">
              <p className="font-medium text-foreground/70">
                MQL5 Notes:
              </p>
              <ul className="list-inside list-disc space-y-0.5 pl-1">
                <li>
                  All indicators use custom implementations matching the
                  backtester engine exactly
                </li>
                <li>
                  Place the BT_*.mq5 indicator files in your{" "}
                  <code className="rounded bg-muted px-1">
                    MQL5/Indicators/
                  </code>{" "}
                  folder
                </li>
                <li>
                  Compile all indicator files and the EA in MetaEditor before
                  using
                </li>
                <li>
                  Adjust the Magic Number input to avoid conflicts with other
                  EAs
                </li>
                <li>
                  Test in Strategy Tester before deploying to a live account
                </li>
              </ul>
            </div>
          ) : (
            <div className="space-y-1 text-[10px] text-muted-foreground">
              <p className="font-medium text-foreground/70">
                Pine Script Notes:
              </p>
              <ul className="list-inside list-disc space-y-0.5 pl-1">
                <li>
                  Copy the code to TradingView&apos;s Pine Editor and add to
                  chart
                </li>
                <li>
                  Commission and slippage are configured in the strategy()
                  declaration
                </li>
                <li>
                  Position sizing may need manual adjustment for your account
                  size
                </li>
                <li>
                  ADX uses ta.dmi() which may behave slightly differently
                </li>
                <li>
                  Backtest results will differ due to platform differences in
                  order execution
                </li>
              </ul>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
