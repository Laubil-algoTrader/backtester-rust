import { useState, useMemo } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";

const MONTH_NAMES = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

const DAY_NAMES = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

interface CalendarProps {
  selected?: Date;
  onSelect?: (date: Date) => void;
  defaultMonth?: Date;
}

export function Calendar({ selected, onSelect, defaultMonth }: CalendarProps) {
  const initial = defaultMonth ?? selected ?? new Date();
  const [viewYear, setViewYear] = useState(initial.getFullYear());
  const [viewMonth, setViewMonth] = useState(initial.getMonth());

  // Generate calendar grid for current month
  const days = useMemo(() => {
    const firstDay = new Date(viewYear, viewMonth, 1).getDay();
    const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate();
    const daysInPrevMonth = new Date(viewYear, viewMonth, 0).getDate();

    const cells: { day: number; month: number; year: number; outside: boolean }[] = [];

    // Previous month fill
    for (let i = firstDay - 1; i >= 0; i--) {
      const d = daysInPrevMonth - i;
      const m = viewMonth === 0 ? 11 : viewMonth - 1;
      const y = viewMonth === 0 ? viewYear - 1 : viewYear;
      cells.push({ day: d, month: m, year: y, outside: true });
    }

    // Current month
    for (let d = 1; d <= daysInMonth; d++) {
      cells.push({ day: d, month: viewMonth, year: viewYear, outside: false });
    }

    // Next month fill (to complete 6 rows)
    const remaining = 42 - cells.length;
    for (let d = 1; d <= remaining; d++) {
      const m = viewMonth === 11 ? 0 : viewMonth + 1;
      const y = viewMonth === 11 ? viewYear + 1 : viewYear;
      cells.push({ day: d, month: m, year: y, outside: true });
    }

    return cells;
  }, [viewYear, viewMonth]);

  const isSelected = (cell: { day: number; month: number; year: number }) => {
    if (!selected) return false;
    return (
      selected.getFullYear() === cell.year &&
      selected.getMonth() === cell.month &&
      selected.getDate() === cell.day
    );
  };

  const isToday = (cell: { day: number; month: number; year: number }) => {
    const now = new Date();
    return (
      now.getFullYear() === cell.year &&
      now.getMonth() === cell.month &&
      now.getDate() === cell.day
    );
  };

  const handlePrevMonth = () => {
    if (viewMonth === 0) {
      setViewMonth(11);
      setViewYear(viewYear - 1);
    } else {
      setViewMonth(viewMonth - 1);
    }
  };

  const handleNextMonth = () => {
    if (viewMonth === 11) {
      setViewMonth(0);
      setViewYear(viewYear + 1);
    } else {
      setViewMonth(viewMonth + 1);
    }
  };

  const handleDayClick = (cell: { day: number; month: number; year: number }) => {
    onSelect?.(new Date(cell.year, cell.month, cell.day));
  };

  // Year range for dropdown
  const currentYear = new Date().getFullYear();
  const years = useMemo(() => {
    const list: number[] = [];
    for (let y = 2000; y <= currentYear + 2; y++) {
      list.push(y);
    }
    return list;
  }, [currentYear]);

  return (
    <div className="w-[252px] select-none bg-card p-3">
      {/* Header: prev arrow | month name + year dropdown | next arrow */}
      <div className="mb-2 flex items-center justify-between">
        <button
          type="button"
          onClick={handlePrevMonth}
          className="inline-flex h-7 w-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
        >
          <ChevronLeft className="h-4 w-4" />
        </button>

        <div className="flex items-center gap-1">
          {/* Month selector */}
          <select
            value={viewMonth}
            onChange={(e) => setViewMonth(Number(e.target.value))}
            className="cursor-pointer appearance-none rounded bg-transparent px-1 py-0.5 text-sm font-medium text-foreground hover:bg-muted focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {MONTH_NAMES.map((name, i) => (
              <option key={i} value={i} className="bg-card text-foreground">
                {name}
              </option>
            ))}
          </select>

          {/* Year selector */}
          <select
            value={viewYear}
            onChange={(e) => setViewYear(Number(e.target.value))}
            className="cursor-pointer appearance-none rounded bg-transparent px-1 py-0.5 text-sm font-medium text-foreground hover:bg-muted focus:outline-none focus:ring-1 focus:ring-ring"
          >
            {years.map((y) => (
              <option key={y} value={y} className="bg-card text-foreground">
                {y}
              </option>
            ))}
          </select>
        </div>

        <button
          type="button"
          onClick={handleNextMonth}
          className="inline-flex h-7 w-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground"
        >
          <ChevronRight className="h-4 w-4" />
        </button>
      </div>

      {/* Weekday headers */}
      <div className="mb-1 grid grid-cols-7">
        {DAY_NAMES.map((d) => (
          <div
            key={d}
            className="flex h-8 w-8 items-center justify-center text-xs font-medium text-muted-foreground"
          >
            {d}
          </div>
        ))}
      </div>

      {/* Day grid */}
      <div className="grid grid-cols-7">
        {days.map((cell, i) => {
          const sel = isSelected(cell);
          const today = isToday(cell);
          return (
            <button
              key={i}
              type="button"
              onClick={() => handleDayClick(cell)}
              className={`flex h-8 w-8 items-center justify-center rounded text-sm transition-colors
                ${cell.outside ? "text-muted-foreground/30" : "text-foreground"}
                ${sel ? "bg-primary text-primary-foreground hover:bg-primary" : "hover:bg-muted"}
                ${today && !sel ? "font-bold text-primary" : ""}
              `}
            >
              {cell.day}
            </button>
          );
        })}
      </div>
    </div>
  );
}
