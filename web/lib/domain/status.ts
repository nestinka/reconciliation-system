import {
  CircleCheck,
  CircleAlert,
  CircleX,
  Clock,
  Eye,
  Ban,
  CircleMinus,
  Loader,
  CircleCheckBig,
  Info,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export type StatusKind =
  | "matched"
  | "partial"
  | "unmatched"
  | "break"
  | "duplicate"
  | "open"
  | "investigating"
  | "pending_approval"
  | "resolved"
  | "written_off"
  | "running"
  | "completed"
  | "failed";

export type Tone = "success" | "warning" | "danger" | "info" | "neutral";

type StatusMeta = { tone: Tone; label: string; icon: LucideIcon };

const STATUS_MAP: Record<StatusKind, StatusMeta> = {
  matched: {
    tone: "success",
    label: "Matched",
    icon: CircleCheck,
  },
  partial: {
    tone: "warning",
    label: "Partial",
    icon: CircleAlert,
  },
  unmatched: {
    tone: "danger",
    label: "Unmatched",
    icon: CircleX,
  },
  break: {
    tone: "danger",
    label: "Break",
    icon: Ban,
  },
  duplicate: {
    tone: "warning",
    label: "Duplicate",
    icon: CircleAlert,
  },
  open: {
    tone: "neutral",
    label: "Open",
    icon: CircleMinus,
  },
  investigating: {
    tone: "info",
    label: "Investigating",
    icon: Eye,
  },
  pending_approval: {
    tone: "warning",
    label: "Pending Approval",
    icon: Clock,
  },
  resolved: {
    tone: "success",
    label: "Resolved",
    icon: CircleCheckBig,
  },
  written_off: {
    tone: "neutral",
    label: "Written Off",
    icon: Ban,
  },
  running: {
    tone: "info",
    label: "Running",
    icon: Loader,
  },
  completed: {
    tone: "success",
    label: "Completed",
    icon: CircleCheck,
  },
  failed: {
    tone: "danger",
    label: "Failed",
    icon: Info,
  },
};

export function statusMeta(kind: StatusKind): StatusMeta {
  return STATUS_MAP[kind];
}
