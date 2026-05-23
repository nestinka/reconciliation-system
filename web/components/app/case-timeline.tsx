import {
  MessageSquare,
  UserCheck,
  Shuffle,
  FileX,
  Clock,
  CheckCircle2,
  XCircle,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatDateTime } from "@/lib/domain/date";
import type { CaseEvent, User } from "@/lib/domain/types";

interface CaseTimelineProps {
  events: CaseEvent[];
  usersById: Record<string, User>;
}

interface EventMeta {
  Icon: LucideIcon;
  iconClass: string;
  description: (event: CaseEvent, usersById: Record<string, User>) => string;
}

const EVENT_META: Record<CaseEvent["kind"], EventMeta> = {
  comment: {
    Icon: MessageSquare,
    iconClass: "text-muted-foreground",
    description: (e) =>
      e.kind === "comment" ? `"${e.payload.text}"` : "",
  },
  assignment: {
    Icon: UserCheck,
    iconClass: "text-info",
    description: (e, usersById) => {
      if (e.kind !== "assignment") return "";
      const name =
        usersById[e.payload.assigneeId]?.name ?? e.payload.assigneeId;
      return `Assigned to ${name}`;
    },
  },
  manual_match_proposed: {
    Icon: Shuffle,
    iconClass: "text-warning",
    description: () => "Proposed a manual match",
  },
  write_off_proposed: {
    Icon: FileX,
    iconClass: "text-warning",
    description: (e) =>
      e.kind === "write_off_proposed"
        ? `Proposed a write-off: ${e.payload.reason}`
        : "",
  },
  approval_requested: {
    Icon: Clock,
    iconClass: "text-warning",
    description: (e) =>
      e.kind === "approval_requested"
        ? `Requested approval (${e.payload.resolution.replace(/_/g, " ")})`
        : "",
  },
  approved: {
    Icon: CheckCircle2,
    iconClass: "text-success",
    description: () => "Approved the resolution",
  },
  rejected: {
    Icon: XCircle,
    iconClass: "text-danger",
    description: (e) =>
      e.kind === "rejected" ? `Rejected: ${e.payload.reason}` : "",
  },
};

function resolveActorName(actorId: string, usersById: Record<string, User>): string {
  return usersById[actorId]?.name ?? actorId;
}

export function CaseTimeline({ events, usersById }: CaseTimelineProps) {
  if (events.length === 0) {
    return (
      <p className="text-xs text-muted-foreground italic py-3">
        No activity yet.
      </p>
    );
  }

  return (
    <ol
      aria-label="Case activity timeline"
      className="relative flex flex-col gap-0"
    >
      {events.map((event, index) => {
        const meta = EVENT_META[event.kind];
        const { Icon, iconClass } = meta;
        const description = meta.description(event, usersById);
        const actorName = resolveActorName(event.actorId, usersById);
        const isLast = index === events.length - 1;

        return (
          <li key={event.id} className="relative flex gap-3 pb-4">
            {/* Vertical connector line */}
            {!isLast && (
              <span
                aria-hidden
                className="absolute left-[13px] top-6 bottom-0 w-px bg-border"
              />
            )}

            {/* Icon dot */}
            <span
              aria-hidden
              className="relative z-10 mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full bg-card ring-1 ring-border"
            >
              <Icon className={cn("size-3.5", iconClass)} />
            </span>

            {/* Content */}
            <div className="flex min-w-0 flex-1 flex-col gap-0.5 pt-0.5">
              <div className="flex flex-wrap items-baseline gap-1.5">
                <span className="text-xs font-medium text-foreground">
                  {actorName}
                </span>
                <time
                  dateTime={event.at}
                  className="text-xs text-muted-foreground"
                >
                  {formatDateTime(event.at)}
                </time>
              </div>
              <p
                className={`text-xs leading-relaxed ${
                  event.kind === "comment"
                    ? "italic text-muted-foreground"
                    : "text-foreground/80"
                }`}
              >
                {description}
              </p>
            </div>
          </li>
        );
      })}
    </ol>
  );
}
