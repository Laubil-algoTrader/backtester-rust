# Design: Dukascopy "Via CSV" Download Pipeline Option

**Date:** 2026-03-12
**Status:** Approved

---

## Problem

Users are seeing large discrepancies between results from directly-downloaded tick data (bi5 → Parquet/Binary) and manually-imported CSV data (Dukascopy CSV → Parquet/Binary). The CSV import path is the reference/correct path. Adding a "Via CSV" pipeline option guarantees 100% parity with the manual CSV import.

---

## Solution

Add a `ViaCsv` pipeline option for tick-mode downloads that routes through the existing `download_symbol()` + `stream_tick_csv_to_parquet()` functions — the exact same code path as manual CSV import — before writing to Parquet/Binary storage.

---

## Architecture

### Data Flow — Direct (existing)
```
bi5 → parse_bi5() → YearBuffer → flush_year_buffer() → Parquet/Binary
```

### Data Flow — Via CSV (new)
```
bi5 → parse_bi5() → raw_ticks.csv → stream_tick_csv_to_parquet() → Parquet/Binary
                                              ↓ (if keep_csv=false)
                                         [deleted]
```

---

## Backend Changes

### `models/config.rs`
Add enum:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TickPipeline {
    #[default]
    Direct,
    ViaCsv,
}
```

### `commands.rs` — `download_dukascopy`
Add two optional parameters:
- `pipeline: Option<TickPipeline>` — defaults to `Direct`
- `keep_csv: Option<bool>` — defaults to `false`; only used when `pipeline = ViaCsv`

When `pipeline = ViaCsv` (tick mode only):
1. Call `download_symbol(duka_symbol, point_value, start, end, csv_path, cancel, progress)` → writes `data/symbols/<name>/raw_ticks.csv`
2. Call `stream_tick_csv_to_parquet(csv_path, tick_dir, tick_raw_dir, format, cancel, progress)`
3. If `!keep_csv` → `std::fs::remove_file(csv_path)`
4. Continue with `generate_timeframes_from_partitions()` as normal

M1 mode always uses the existing direct M1 aggregation — pipeline option is ignored.

Intermediate CSV path: `<symbol_dir>/raw_ticks.csv`

---

## Frontend Changes

### `src/lib/types.ts`
Add type:
```typescript
export type TickPipeline = "direct" | "via_csv";
```

### `src/lib/tauri.ts`
Update `downloadDukascopy` signature to accept `pipeline?: TickPipeline` and `keep_csv?: boolean`.

### `src/components/data/DownloadDialog.tsx`
In the tick-mode section, below "Storage Format", add a **Pipeline** selector:

```
Pipeline
  [● Direct (fast)    ] [○ Via CSV (parity)]
```

When "Via CSV" is selected, show:
```
  ☐ Keep intermediate CSV after conversion
```

- Block is hidden when modeling = `m1`
- Default: `Direct`
- Tooltip on "Via CSV": "Same pipeline as manual CSV import — use if you see discrepancies"

---

## Key Constraints

- `ViaCsv` only applies to `modeling = "tick"` — M1 downloads always use the direct M1 aggregator
- `keep_csv` is only sent when `pipeline = "via_csv"`
- CSV intermediate path is always `<symbol_dir>/raw_ticks.csv` — never configurable
- No changes to `stream_tick_csv_to_parquet()` or `download_symbol()` — pure orchestration
- Cancellation works: `cancel_flag` is passed to both `download_symbol()` and `stream_tick_csv_to_parquet()`

---

## Files to Change

| File | Change |
|---|---|
| `src-tauri/src/models/config.rs` | Add `TickPipeline` enum |
| `src-tauri/src/commands.rs` | Add `pipeline` + `keep_csv` params, route logic |
| `src/lib/types.ts` | Add `TickPipeline` type |
| `src/lib/tauri.ts` | Update `downloadDukascopy` wrapper signature |
| `src/components/data/DownloadDialog.tsx` | Add Pipeline UI block |
