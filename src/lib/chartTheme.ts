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
