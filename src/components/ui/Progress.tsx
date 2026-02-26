import { cn } from "@/lib/utils";

interface ProgressProps {
  value: number;
  className?: string;
  indicatorClassName?: string;
}

function Progress({ value, className, indicatorClassName }: ProgressProps) {
  return (
    <div
      className={cn(
        "relative h-1.5 w-full overflow-hidden rounded bg-secondary",
        className
      )}
    >
      <div
        className={cn(
          "h-full rounded bg-primary transition-all duration-300 ease-in-out",
          indicatorClassName
        )}
        style={{ width: `${Math.min(100, Math.max(0, value))}%` }}
      />
    </div>
  );
}

export { Progress };
