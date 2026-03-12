# Via CSV Pipeline Option — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a "Via CSV" pipeline option for tick-mode Dukascopy downloads that routes through the existing `download_symbol()` + `stream_tick_csv_to_parquet()` functions, guaranteeing 100% parity with manual CSV imports.

**Architecture:** New `TickPipeline` enum (Direct | ViaCsv) added to `models/config.rs`. Two optional params added to the `download_dukascopy` Tauri command. When `ViaCsv` is selected, the command orchestrates the existing `download_symbol()` → `stream_tick_csv_to_parquet()` → optional CSV cleanup. No changes to core download or parsing functions — pure orchestration. Frontend adds a Pipeline toggle (Direct / Via CSV) and a "Keep CSV" checkbox visible only in tick + ViaCsv mode.

**Tech Stack:** Rust (Tauri 2, serde), TypeScript, React, Tailwind CSS

---

## Task 1: Add `TickPipeline` enum to Rust models

**Files:**
- Modify: `src-tauri/src/models/config.rs` (after line 200, after `TickStorageFormat` enum)

**Step 1: Add the enum**

Add this block immediately after the closing `}` of `TickStorageFormat` (around line 200):

```rust
/// Download pipeline for tick-mode Dukascopy downloads.
///
/// `Direct` (default): bi5 → YearBuffer → Parquet/Binary — fast, no intermediate files.
/// `ViaCsv`: bi5 → intermediate CSV → `stream_tick_csv_to_parquet()` — identical
/// to the manual CSV import path; use when the Direct path produces discrepancies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TickPipeline {
    #[default]
    Direct,
    ViaCsv,
}
```

**Step 2: Run `cargo check`**

```bat
cargo_check.bat
```
Expected: `Finished` with no errors.

**Step 3: Commit**

```bash
git add src-tauri/src/models/config.rs
git commit -m "feat: add TickPipeline enum (Direct | ViaCsv)"
```

---

## Task 2: Update `download_dukascopy` command with Via CSV routing

**Files:**
- Modify: `src-tauri/src/commands.rs` (function starting at line 961)

**Step 1: Add imports at the top of the tick-mode block**

The function already imports `crate::data::dukascopy`. Ensure `crate::data::{loader, validator}` are also imported within the function. They're already used elsewhere in commands.rs — no new `use` at file top needed, just inside the function.

**Step 2: Add two new parameters to `download_dukascopy`**

Change the function signature (currently ends at line 971):

```rust
pub async fn download_dukascopy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    symbol_name: String,
    duka_symbol: String,
    point_value: f64,
    start_date: String,
    end_date: String,
    base_timeframe: String,
    instrument_config: InstrumentConfig,
    tick_storage_format: Option<TickStorageFormat>,
    tick_pipeline: Option<TickPipeline>,   // NEW
    keep_csv: Option<bool>,                // NEW
) -> Result<Symbol, AppError> {
```

Also add to the `use crate::...` block at the top of the function body:
```rust
use crate::models::config::TickPipeline;
```

**Step 3: Replace the tick-mode block with pipeline routing**

Find the tick-mode block (currently lines 1016–1047):

```rust
let (total_rows, data_start, data_end, timeframe_paths, final_base_tf) = if is_tick_mode {
    // ── Tick mode: write raw ticks directly to Parquet/Binary, then generate timeframes ──
    let tick_dir = symbol_dir.join("tick");
    let tick_raw_dir = symbol_dir.join("tick_raw");
    // ...calls download_symbol_direct...
```

Replace the entire tick-mode arm (from `if is_tick_mode {` through the matching `}` before `} else {`) with:

```rust
let (total_rows, data_start, data_end, timeframe_paths, final_base_tf) = if is_tick_mode {
    let tick_dir = symbol_dir.join("tick");
    let tick_raw_dir = symbol_dir.join("tick_raw");

    let pipeline = tick_pipeline.unwrap_or_default();

    let (total_rows, data_start, data_end) = match pipeline {
        TickPipeline::Direct => {
            // ── Direct path: bi5 → YearBuffer → Parquet/Binary ──────────────────
            let app_clone = app.clone();
            let sym_clone = symbol_name.clone();
            dukascopy::download_symbol_direct(
                &duka_symbol,
                point_value,
                start,
                end,
                &tick_dir,
                &tick_raw_dir,
                tick_storage_format,
                instrument_config.tz_offset_hours,
                &cancel_flag,
                move |pct, msg| {
                    let mapped = (pct as f64 * 0.92) as u8;
                    emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                },
            ).await?
        }

        TickPipeline::ViaCsv => {
            // ── Via CSV path: bi5 → raw_ticks.csv → stream_tick_csv_to_parquet ──
            let csv_path = symbol_dir.join("raw_ticks.csv");

            // Phase 1: download bi5 → CSV
            {
                let app_clone = app.clone();
                let sym_clone = symbol_name.clone();
                dukascopy::download_symbol(
                    &duka_symbol,
                    point_value,
                    start,
                    end,
                    &csv_path,
                    &cancel_flag,
                    move |pct, msg| {
                        // Download uses 0–70% of total progress
                        let mapped = (pct as f64 * 0.60) as u8;
                        emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                    },
                ).await?;
            }

            emit_download_progress(&app, &symbol_name, 62, "Converting CSV to storage format...");

            // Phase 2: CSV → Parquet/Binary (same as manual import)
            // download_symbol() always writes: DateTime,Bid,Ask,Volume (4 cols, with header)
            let validation = validator::ValidationResult {
                format: crate::models::config::DataFormat::Tick,
                has_header: true,
                delimiter: b',',
                row_count_sample: 0,
                column_count: 4,
            };

            let app_clone = app.clone();
            let sym_clone = symbol_name.clone();
            let (total_rows, data_start, data_end) = loader::stream_tick_csv_to_parquet(
                &csv_path,
                &validation,
                &tick_dir,
                &tick_raw_dir,
                tick_storage_format,
                instrument_config.tz_offset_hours,
                move |pct, msg| {
                    // Conversion uses 62–92% of total progress
                    let mapped = 62 + (pct as f64 * 0.30) as u8;
                    emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                },
            )?;

            // Phase 3: optionally remove the intermediate CSV
            if !keep_csv.unwrap_or(false) {
                if let Err(e) = std::fs::remove_file(&csv_path) {
                    warn!("Could not remove intermediate CSV {}: {}", csv_path.display(), e);
                }
            } else {
                info!("Keeping intermediate CSV at {}", csv_path.display());
            }

            (total_rows, data_start, data_end)
        }
    };

    emit_download_progress(&app, &symbol_name, 93, "Generating timeframes...");
    let mut timeframe_paths = converter::generate_timeframes_from_partitions(
        &tick_dir,
        &symbol_dir,
    )?;
    timeframe_paths.insert("tick".into(), tick_dir.to_string_lossy().into());
    timeframe_paths.insert("tick_raw".into(), tick_raw_dir.to_string_lossy().into());

    (total_rows, data_start, data_end, timeframe_paths, Timeframe::Tick)
```

**Note:** `validator::ValidationResult` fields are `pub`, so they can be constructed directly. The struct is in `crate::data::validator` which is already in scope in commands.rs.

**Step 4: Run `cargo check`**

```bat
cargo_check.bat
```
Expected: `Finished` with no errors. If you get "field `X` is private", add `pub` to any private field in `validator::ValidationResult` (check `src-tauri/src/data/validator.rs` lines 10–16 — all fields should already be `pub`).

**Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat: add ViaCsv pipeline routing to download_dukascopy command"
```

---

## Task 3: Add `TickPipeline` type to TypeScript

**Files:**
- Modify: `src/lib/types.ts` (after `TickStorageFormat` type, around line 153)

**Step 1: Add the type**

After the line:
```typescript
export type TickStorageFormat = "Parquet" | "Binary";
```

Add:
```typescript
/** Download pipeline for tick-mode Dukascopy downloads.
 *  - "direct"  → bi5 → Parquet/Binary (fast, default)
 *  - "via_csv" → bi5 → CSV → Parquet/Binary (parity with manual import)
 */
export type TickPipeline = "direct" | "via_csv";
```

**Step 2: Run TypeScript check**

```bash
npx tsc --noEmit
```
Expected: no output (clean).

**Step 3: Commit**

```bash
git add src/lib/types.ts
git commit -m "feat: add TickPipeline TypeScript type"
```

---

## Task 4: Update `downloadDukascopy` wrapper in `tauri.ts`

**Files:**
- Modify: `src/lib/tauri.ts` (function at line 157)

**Step 1: Add import of `TickPipeline`**

Update the import block (lines 1–17):
```typescript
import type {
  // ...existing...
  TickStorageFormat,
  TickPipeline,       // ADD THIS
  // ...rest...
} from "./types";
```

**Step 2: Update `downloadDukascopy` signature and body**

Replace the function (lines 157–177):
```typescript
/// Download historical tick data from Dukascopy servers.
export async function downloadDukascopy(
  symbolName: string,
  dukaSymbol: string,
  pointValue: number,
  startDate: string,
  endDate: string,
  baseTimeframe: "tick" | "m1",
  instrumentConfig: InstrumentConfig,
  tickStorageFormat?: TickStorageFormat,
  tickPipeline?: TickPipeline,   // NEW
  keepCsv?: boolean              // NEW
): Promise<Symbol> {
  return invoke<Symbol>("download_dukascopy", {
    symbolName,
    dukaSymbol,
    pointValue,
    startDate,
    endDate,
    baseTimeframe,
    instrumentConfig,
    tickStorageFormat,
    tickPipeline,    // NEW
    keepCsv,         // NEW
  });
}
```

**Step 3: Run TypeScript check**

```bash
npx tsc --noEmit
```
Expected: clean.

**Step 4: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: add tickPipeline and keepCsv params to downloadDukascopy wrapper"
```

---

## Task 5: Add Pipeline UI to `DownloadDialog.tsx`

**Files:**
- Modify: `src/components/data/DownloadDialog.tsx`

**Step 1: Add state variables**

After the existing `const [storageFormat, setStorageFormat] = ...` line, add:
```typescript
// Pipeline (only for tick mode)
const [pipeline, setPipeline] = useState<TickPipeline>("direct");
const [keepCsv, setKeepCsv] = useState(false);
```

Also add `TickPipeline` to the import from `@/lib/types`:
```typescript
import type {
  InstrumentConfig,
  DukascopyCategory,
  DukascopyInstrument,
  TickStorageFormat,
  TickPipeline,      // ADD
} from "@/lib/types";
```

**Step 2: Pass pipeline params to `handleDownload`**

Update the `downloadDukascopy(...)` call inside `handleDownload` to include the two new arguments:
```typescript
downloadDukascopy(
  symbolName,
  effectiveSymbol,
  effectivePointValue,
  startDate,
  endDate,
  modeling,
  config,
  modeling === "tick" ? storageFormat : undefined,
  modeling === "tick" ? pipeline : undefined,     // NEW
  modeling === "tick" && pipeline === "via_csv" ? keepCsv : undefined  // NEW
)
```

**Step 3: Add Pipeline UI block in JSX**

Place this block immediately **after** the Storage Format section (the `{modeling === "tick" && (...)}` block). It should also only render for tick mode:

```tsx
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
```

**Step 4: Add i18n keys**

The app uses i18next. Add these keys to the translation file(s). Find them with:
```bash
grep -r "downloadDialog.tickData" src/i18n/
```
Then open the found file(s) and add inside the `downloadDialog` object:
```json
"pipeline": "Pipeline",
"pipelineDirect": "Direct (fast)",
"pipelineViaCsv": "Via CSV (parity)",
"pipelineDirectDesc": "Downloads bi5 data directly to storage — fastest option.",
"pipelineViaCsvDesc": "Routes through an intermediate CSV — identical to manual CSV import. Use if Direct results differ.",
"keepCsv": "Keep intermediate CSV after conversion"
```

**Step 5: Run TypeScript check**

```bash
npx tsc --noEmit
```
Expected: clean.

**Step 6: Commit**

```bash
git add src/components/data/DownloadDialog.tsx src/i18n/
git commit -m "feat: add Via CSV pipeline toggle and Keep CSV checkbox to DownloadDialog"
```

---

## Task 6: Final verification

**Step 1: Full Rust build**

```bat
cargo_check.bat
```
Expected: `Finished` with no errors or warnings.

**Step 2: Full TypeScript check**

```bash
npx tsc --noEmit
```
Expected: clean.

**Step 3: Manual smoke test**

Launch the app with `cargo tauri dev` and:
1. Open Download Dialog → select a tick instrument (e.g. EURUSD)
2. Set modeling = `tick`
3. Verify the **Pipeline** block appears with "Direct (fast)" and "Via CSV (parity)" buttons
4. Select "Via CSV" → verify the "Keep intermediate CSV" checkbox appears
5. Select "Direct" → verify the checkbox disappears
6. Switch to `m1` modeling → verify the Pipeline block disappears entirely

**Step 4: Commit**

```bash
git add -A
git commit -m "chore: final verification pass for Via CSV pipeline feature"
```
