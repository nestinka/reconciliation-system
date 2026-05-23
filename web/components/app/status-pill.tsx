import { statusMeta, type StatusKind, type Tone } from "@/lib/domain/status";
import { cn } from "@/lib/utils";

const TONE_CLASSES: Record<Tone, string> = {
  success: "bg-success/10 text-success border border-success/20",
  warning: "bg-warning/10 text-warning border border-warning/20",
  danger: "bg-danger/10 text-danger border border-danger/20",
  info: "bg-info/10 text-info border border-info/20",
  neutral: "bg-neutral/10 text-neutral border border-neutral/20",
};

interface StatusPillProps {
  status: StatusKind;
  className?: string;
}

export function StatusPill({ status, className }: StatusPillProps) {
  const { tone, label, icon: Icon } = statusMeta(status);
  const toneClasses = TONE_CLASSES[tone];

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium leading-tight",
        toneClasses,
        className
      )}
    >
      <Icon aria-hidden className="size-3 shrink-0" />
      {label}
    </span>
  );
}
