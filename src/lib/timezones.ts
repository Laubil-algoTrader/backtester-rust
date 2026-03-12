// ── Timezone options for import / download dialogs ──
//
// Each entry represents a UTC offset (in decimal hours).
// Fractional offsets are supported (e.g. 5.5 = UTC+5:30, 5.75 = UTC+5:45).

export interface TimezoneOption {
  /** Human-readable label shown in the dropdown */
  label: string;
  /** UTC offset in decimal hours (positive = east, negative = west) */
  value: number;
}

export const TIMEZONE_OPTIONS: TimezoneOption[] = [
  { label: "UTC-12 (Baker Island)", value: -12 },
  { label: "UTC-11 (American Samoa)", value: -11 },
  { label: "UTC-10 (Hawaii)", value: -10 },
  { label: "UTC-9 (Alaska)", value: -9 },
  { label: "UTC-8 (Los Angeles, Vancouver, PST)", value: -8 },
  { label: "UTC-7 (Denver, Phoenix, MST)", value: -7 },
  { label: "UTC-6 (Chicago, Mexico City, CST)", value: -6 },
  { label: "UTC-5 (New York, Toronto, Lima, EST)", value: -5 },
  { label: "UTC-4 (Santiago, Caracas, AST)", value: -4 },
  { label: "UTC-3:30 (Newfoundland)", value: -3.5 },
  { label: "UTC-3 (Buenos Aires, São Paulo, BRT)", value: -3 },
  { label: "UTC-2 (South Georgia)", value: -2 },
  { label: "UTC-1 (Azores, Cape Verde)", value: -1 },
  { label: "UTC+0 (London, Lisbon, Reykjavik, UTC)", value: 0 },
  { label: "UTC+1 (Paris, Berlin, Madrid, Rome)", value: 1 },
  { label: "UTC+2 (Helsinki, Athens, Cairo, CEST)", value: 2 },
  { label: "UTC+3 (Moscow, Istanbul, Riyadh, MSK)", value: 3 },
  { label: "UTC+3:30 (Tehran, IRST)", value: 3.5 },
  { label: "UTC+4 (Dubai, Baku, Gulf)", value: 4 },
  { label: "UTC+4:30 (Kabul)", value: 4.5 },
  { label: "UTC+5 (Karachi, Tashkent)", value: 5 },
  { label: "UTC+5:30 (Mumbai, New Delhi, IST)", value: 5.5 },
  { label: "UTC+5:45 (Kathmandu)", value: 5.75 },
  { label: "UTC+6 (Dhaka, Almaty)", value: 6 },
  { label: "UTC+6:30 (Yangon)", value: 6.5 },
  { label: "UTC+7 (Bangkok, Jakarta, Hanoi)", value: 7 },
  { label: "UTC+8 (Beijing, Singapore, Hong Kong, Perth)", value: 8 },
  { label: "UTC+9 (Tokyo, Seoul, Osaka, JST)", value: 9 },
  { label: "UTC+9:30 (Adelaide)", value: 9.5 },
  { label: "UTC+10 (Sydney, Melbourne, Brisbane, AEST)", value: 10 },
  { label: "UTC+11 (Solomon Islands)", value: 11 },
  { label: "UTC+12 (Auckland, Fiji)", value: 12 },
  { label: "UTC+13 (Samoa DST)", value: 13 },
  { label: "UTC+14 (Kiribati)", value: 14 },
];

/** Format a numeric offset as a compact UTC±H string, e.g. +3 → "UTC+3", -3.5 → "UTC-3:30" */
export function formatTzOffset(hours: number): string {
  if (hours === 0) return "UTC+0";
  const sign = hours > 0 ? "+" : "-";
  const abs = Math.abs(hours);
  if (abs % 1 === 0) {
    return `UTC${sign}${abs}`;
  }
  const h = Math.floor(abs);
  const m = Math.round((abs % 1) * 60)
    .toString()
    .padStart(2, "0");
  return `UTC${sign}${h}:${m}`;
}

/** Find the best-matching TimezoneOption label for a given offset value. */
export function tzLabel(hours: number): string {
  const exact = TIMEZONE_OPTIONS.find((o) => o.value === hours);
  return exact ? exact.label : formatTzOffset(hours);
}
