/** Shared chart styling constants matching the dark financial dashboard aesthetic */

export const CHART_COLORS = {
  green: "hsl(152 60% 42%)",
  red: "hsl(0 72% 50%)",
  blue: "hsl(210 80% 55%)",
  amber: "hsl(38 92% 50%)",
  purple: "hsl(280 65% 55%)",
} as const;

export const GRID_COLOR = "hsl(0 0% 11%)";
export const GRID_DASH = "3 6";

export const AXIS_TICK = { fontSize: 12, fill: "hsl(0 0% 38%)" };
export const AXIS_STROKE = "hsl(0 0% 11%)";

export const TOOLTIP_STYLE: React.CSSProperties = {
  backgroundColor: "hsl(0 0% 8%)",
  border: "1px solid hsl(0 0% 14%)",
  borderRadius: 6,
  fontSize: 12,
  fontFamily: "'JetBrains Mono', monospace",
  color: "hsl(0 0% 85%)",
};

export const TOOLTIP_LABEL_STYLE: React.CSSProperties = {
  color: "hsl(0 0% 55%)",
  fontSize: 11,
  marginBottom: 4,
};

// Import React types for CSSProperties
import type React from "react";

// ── Per-theme chart constants ────────────────────────────────────────────────

const DARK_CHART_THEME = {
  GRID_COLOR,
  GRID_DASH,
  AXIS_TICK,
  AXIS_STROKE,
  TOOLTIP_STYLE,
  TOOLTIP_LABEL_STYLE,
};

const LIGHT_CHART_THEME = {
  GRID_COLOR: "hsl(0 0% 90%)",
  GRID_DASH: "3 6",
  AXIS_TICK: { fontSize: 12, fill: "hsl(0 0% 45%)" },
  AXIS_STROKE: "hsl(0 0% 90%)",
  TOOLTIP_STYLE: {
    backgroundColor: "hsl(0 0% 100%)",
    border: "1px solid hsl(0 0% 85%)",
    borderRadius: 6,
    fontSize: 12,
    fontFamily: "'JetBrains Mono', monospace",
    color: "hsl(222 84% 5%)",
  } as React.CSSProperties,
  TOOLTIP_LABEL_STYLE: {
    color: "hsl(0 0% 40%)",
    fontSize: 11,
    marginBottom: 4,
  } as React.CSSProperties,
};

const OLYMPUS_CHART_THEME = {
  GRID_COLOR: "hsl(195 15% 11%)",
  GRID_DASH: "3 6",
  AXIS_TICK: { fontSize: 12, fill: "hsl(195 8% 44%)" },
  AXIS_STROKE: "hsl(195 15% 11%)",
  TOOLTIP_STYLE: {
    backgroundColor: "hsl(195 20% 9%)",
    border: "1px solid hsl(195 15% 20%)",
    borderRadius: 6,
    fontSize: 12,
    fontFamily: "'JetBrains Mono', monospace",
    color: "hsl(30 15% 84%)",
  } as React.CSSProperties,
  TOOLTIP_LABEL_STYLE: {
    color: "hsl(195 8% 52%)",
    fontSize: 11,
    marginBottom: 4,
  } as React.CSSProperties,
};

/** Returns chart theme constants matching the current HTML root class. */
export function getChartTheme() {
  if (typeof document === "undefined") return DARK_CHART_THEME;
  const cl = document.documentElement.classList;
  if (cl.contains("olympus")) return OLYMPUS_CHART_THEME;
  if (!cl.contains("dark")) return LIGHT_CHART_THEME;
  return DARK_CHART_THEME;
}
