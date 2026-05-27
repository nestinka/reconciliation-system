"use client";

import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

function truncatePayload(payload: Record<string, unknown>, max = 80): string {
  let str: string;
  try {
    str = JSON.stringify(payload);
  } catch {
    str = String(payload);
  }
  return str.length > max ? `${str.slice(0, max)}…` : str;
}

export interface PayloadCellProps {
  payload: Record<string, unknown>;
}

/** Inline button that shows a truncated payload and opens a full-view dialog. */
export function PayloadCell({ payload }: PayloadCellProps) {
  const [open, setOpen] = useState(false);
  const summary = truncatePayload(payload, 80);
  return (
    <>
      <button
        type="button"
        className="text-left font-mono text-xs hover:underline"
        title="View full payload"
        onClick={() => setOpen(true)}
      >
        {summary}
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Audit payload</DialogTitle>
          </DialogHeader>
          <pre className="max-h-[60vh] overflow-auto rounded-md bg-muted p-3 text-xs">
            {JSON.stringify(payload, null, 2)}
          </pre>
        </DialogContent>
      </Dialog>
    </>
  );
}
