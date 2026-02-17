import { useState } from "react";
import { format, parse, isValid } from "date-fns";
import { CalendarIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "./Button";
import { Calendar } from "./Calendar";
import { Popover, PopoverContent, PopoverTrigger } from "./Popover";

interface DatePickerProps {
  /** Date value as "YYYY-MM-DD" string */
  value: string;
  /** Called with "YYYY-MM-DD" string */
  onChange: (value: string) => void;
  disabled?: boolean;
  className?: string;
  placeholder?: string;
}

export function DatePicker({
  value,
  onChange,
  disabled = false,
  className,
  placeholder = "Select date",
}: DatePickerProps) {
  const [open, setOpen] = useState(false);

  // Parse the string value to a Date for the calendar
  const dateValue = value ? parse(value, "yyyy-MM-dd", new Date()) : undefined;
  const selected = dateValue && isValid(dateValue) ? dateValue : undefined;

  const handleSelect = (date: Date) => {
    onChange(format(date, "yyyy-MM-dd"));
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          disabled={disabled}
          className={cn(
            "h-8 w-full justify-start px-3 text-left text-xs font-normal",
            !value && "text-muted-foreground",
            className
          )}
        >
          <CalendarIcon className="mr-2 h-3.5 w-3.5 text-muted-foreground" />
          {selected ? format(selected, "yyyy-MM-dd") : placeholder}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-auto p-0">
        <Calendar
          selected={selected}
          onSelect={handleSelect}
          defaultMonth={selected}
        />
      </PopoverContent>
    </Popover>
  );
}
