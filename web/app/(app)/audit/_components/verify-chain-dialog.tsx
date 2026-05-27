"use client";

import { useState } from "react";
import { CheckCircle2, ShieldCheck, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { VerifyResult } from "@/lib/api/client";

export interface VerifyChainDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function VerifyChainDialog({
  open,
  onOpenChange,
}: VerifyChainDialogProps) {
  const api = useApi();
  const { tenantId } = useTenant();
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<VerifyResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleRun() {
    setRunning(true);
    setError(null);
    setResult(null);
    try {
      const body: { from?: number; to?: number } = {};
      const fromNum = from.trim() === "" ? undefined : Number(from);
      const toNum = to.trim() === "" ? undefined : Number(to);
      if (fromNum !== undefined && !Number.isNaN(fromNum)) body.from = fromNum;
      if (toNum !== undefined && !Number.isNaN(toNum)) body.to = toNum;
      const r = await api.verifyAudit(tenantId, body);
      setResult(r);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Verification failed.");
    } finally {
      setRunning(false);
    }
  }

  function handleClose(next: boolean) {
    if (!next) {
      // Reset state on close
      setResult(null);
      setError(null);
      setFrom("");
      setTo("");
    }
    onOpenChange(next);
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            <span className="flex items-center gap-2">
              <ShieldCheck aria-hidden className="size-5" />
              Verify audit chain
            </span>
          </DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="verify-from">From seq (optional)</Label>
              <Input
                id="verify-from"
                type="number"
                inputMode="numeric"
                placeholder="e.g. 1"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="verify-to">To seq (optional)</Label>
              <Input
                id="verify-to"
                type="number"
                inputMode="numeric"
                placeholder="e.g. 1000"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </div>
          </div>

          {error && (
            <p className="text-xs text-destructive" role="alert">
              {error}
            </p>
          )}

          {result && (
            <div
              role="status"
              aria-live="polite"
              className="rounded-md border border-border bg-muted/30 p-3 text-sm flex flex-col gap-1"
            >
              <div className="flex items-center gap-2 font-medium">
                {result.status === "valid" ? (
                  <>
                    <CheckCircle2
                      aria-hidden
                      className="size-4 text-emerald-600"
                    />
                    <span className="text-emerald-700 dark:text-emerald-300">
                      Valid
                    </span>
                  </>
                ) : (
                  <>
                    <XCircle aria-hidden className="size-4 text-destructive" />
                    <span className="text-destructive">Invalid</span>
                  </>
                )}
              </div>
              <div className="text-xs text-muted-foreground">
                Checked: <span className="tabular-nums">{result.checked}</span>
              </div>
              {result.status === "invalid" && (
                <>
                  {result.firstBrokenSeq !== undefined && (
                    <div className="text-xs">
                      First broken seq:{" "}
                      <span className="tabular-nums font-mono">
                        {result.firstBrokenSeq}
                      </span>
                    </div>
                  )}
                  {result.reason && (
                    <div className="text-xs">
                      Reason: <code>{result.reason}</code>
                    </div>
                  )}
                </>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => handleClose(false)}
            disabled={running}
          >
            Close
          </Button>
          <Button onClick={handleRun} disabled={running}>
            {running ? "Verifying…" : "Run"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
