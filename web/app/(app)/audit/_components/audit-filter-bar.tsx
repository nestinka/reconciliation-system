"use client";

import { useState } from "react";
import { ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import type { AuditKind } from "@/lib/api/client";

// ---------------------------------------------------------------------------
// AuditKind list (must mirror the union in client.ts — 21 values)
// ---------------------------------------------------------------------------

export const ALL_KINDS: readonly AuditKind[] = [
  "auth.login.success",
  "auth.login.failure",
  "auth.lockout",
  "auth.logout",
  "auth.password.changed",
  "auth.password.reset_requested",
  "auth.password.reset_completed",
  "auth.refresh.reused",
  "auth.tenant.switched",
  "admin.user.created",
  "admin.user.role_changed",
  "admin.user.disabled",
  "admin.user.enabled",
  "admin.user.removed",
  "data.source.created",
  "data.source.updated",
  "data.ingest.completed",
  "data.run.created",
  "case.assigned",
  "case.event_appended",
  "system.anchor.created",
] as const;

// ---------------------------------------------------------------------------
// Event family / colour mapping
// ---------------------------------------------------------------------------

type EventFamily = "auth" | "admin" | "data" | "case" | "system";

export function familyForKind(kind: AuditKind): EventFamily {
  const prefix = kind.split(".")[0] as EventFamily;
  return prefix;
}

export const FAMILY_CLASSES: Record<EventFamily, string> = {
  auth: "bg-blue-500/15 text-blue-700 dark:text-blue-300 border-blue-500/30",
  admin:
    "bg-purple-500/15 text-purple-700 dark:text-purple-300 border-purple-500/30",
  data: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 border-emerald-500/30",
  case: "bg-amber-500/15 text-amber-700 dark:text-amber-300 border-amber-500/30",
  system: "bg-gray-500/15 text-gray-700 dark:text-gray-300 border-gray-500/30",
};

// ---------------------------------------------------------------------------
// Kind multi-select (checkbox list inside a popover-style panel)
// ---------------------------------------------------------------------------

function KindMultiSelect({
  value,
  onChange,
}: {
  value: AuditKind[];
  onChange: (next: AuditKind[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const label =
    value.length === 0
      ? "All event kinds"
      : value.length === 1
        ? value[0]
        : `${value.length} kinds selected`;

  return (
    <div className="relative">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => setOpen((p) => !p)}
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label="Filter by event kind"
      >
        {label}
        <ChevronDown className="size-3.5" />
      </Button>
      {open && (
        <div
          role="listbox"
          aria-label="Event kinds"
          className="absolute z-40 mt-1 max-h-72 w-72 overflow-auto rounded-lg border border-border bg-popover p-2 shadow-md"
        >
          <div className="flex items-center justify-between gap-2 px-1 pb-2">
            <Button
              type="button"
              size="xs"
              variant="ghost"
              onClick={() => onChange([])}
              disabled={value.length === 0}
            >
              Clear
            </Button>
            <Button
              type="button"
              size="xs"
              variant="ghost"
              onClick={() => setOpen(false)}
            >
              Done
            </Button>
          </div>
          <ul className="flex flex-col gap-0.5">
            {ALL_KINDS.map((kind) => {
              const checked = value.includes(kind);
              const family = familyForKind(kind);
              return (
                <li key={kind}>
                  <label className="flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-muted cursor-pointer text-xs">
                    <Checkbox
                      checked={checked}
                      onCheckedChange={(next) => {
                        if (next) onChange([...value, kind]);
                        else onChange(value.filter((k) => k !== kind));
                      }}
                      aria-label={kind}
                    />
                    <span
                      className={`inline-block rounded px-1 py-0.5 border ${FAMILY_CLASSES[family]}`}
                    >
                      {kind}
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// AuditFilterBar
// ---------------------------------------------------------------------------

export interface AuditFilterBarProps {
  kinds: AuditKind[];
  onKindsChange: (v: AuditKind[]) => void;
  actorId: string;
  onActorIdChange: (v: string) => void;
  from: string;
  onFromChange: (v: string) => void;
  to: string;
  onToChange: (v: string) => void;
  canClear: boolean;
  onClearAll: () => void;
}

export function AuditFilterBar({
  kinds,
  onKindsChange,
  actorId,
  onActorIdChange,
  from,
  onFromChange,
  to,
  onToChange,
  canClear,
  onClearAll,
}: AuditFilterBarProps) {
  return (
    <div className="flex items-end gap-3 flex-wrap">
      <div className="flex flex-col gap-1">
        <Label htmlFor="filter-kind">Event kind</Label>
        <KindMultiSelect value={kinds} onChange={onKindsChange} />
      </div>
      <div className="flex flex-col gap-1">
        <Label htmlFor="filter-actor">Actor</Label>
        <Input
          id="filter-actor"
          placeholder="actor id (e.g. user-ada)"
          className="w-52"
          value={actorId}
          onChange={(e) => onActorIdChange(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-1">
        <Label htmlFor="filter-from">From (ISO date)</Label>
        <Input
          id="filter-from"
          type="date"
          className="w-40"
          value={from}
          onChange={(e) => onFromChange(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-1">
        <Label htmlFor="filter-to">To (ISO date)</Label>
        <Input
          id="filter-to"
          type="date"
          className="w-40"
          value={to}
          onChange={(e) => onToChange(e.target.value)}
        />
      </div>
      <Button
        variant="ghost"
        size="sm"
        onClick={onClearAll}
        disabled={!canClear}
      >
        Clear filters
      </Button>
    </div>
  );
}
