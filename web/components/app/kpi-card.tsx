import type { ReactNode } from "react";
import { TrendingUp, TrendingDown, Minus } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { cn } from "@/lib/utils";

interface Delta {
  value: string;
  direction: "up" | "down" | "flat";
}

interface KpiCardProps {
  label: string;
  value: ReactNode;
  hint?: string;
  delta?: Delta;
}

const DELTA_CLASSES: Record<Delta["direction"], string> = {
  up: "text-success",
  down: "text-danger",
  flat: "text-muted-foreground",
};

const DELTA_ICONS: Record<Delta["direction"], typeof TrendingUp> = {
  up: TrendingUp,
  down: TrendingDown,
  flat: Minus,
};

const DELTA_LABELS: Record<Delta["direction"], string> = {
  up: "up",
  down: "down",
  flat: "flat",
};

export function KpiCard({ label, value, hint, delta }: KpiCardProps) {
  return (
    <Card size="sm">
      <CardContent className="flex flex-col gap-1 px-3 py-2">
        <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
          {label}
        </span>
        <span className="nums text-2xl font-semibold leading-none tracking-tight">
          {value}
        </span>
        {(hint || delta) && (
          <div className="flex items-center gap-2 mt-0.5">
            {delta && (() => {
              const DeltaIcon = DELTA_ICONS[delta.direction];
              const colorClass = DELTA_CLASSES[delta.direction];
              return (
                <span className={cn("inline-flex items-center gap-0.5 text-xs font-medium", colorClass)}>
                  <DeltaIcon aria-hidden className="size-3 shrink-0" />
                  <span>{delta.value}</span>
                  <span className="sr-only">({DELTA_LABELS[delta.direction]})</span>
                </span>
              );
            })()}
            {hint && (
              <span className="text-xs text-muted-foreground">{hint}</span>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
