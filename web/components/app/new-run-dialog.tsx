"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useSources } from "@/lib/hooks/use-sources";

export function NewRunDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const api = useApi();
  const router = useRouter();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const { data: sources } = useSources();
  const [name, setName] = useState("");
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

  const mutation = useMutation({
    mutationFn: () =>
      api.createRun(tenantId, { name, sourceAId: a, sourceBId: b, from, to }),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: ["runs", tenantId] });
      toast.success("Run created.");
      onOpenChange(false);
      router.push(`/runs/${run.id}`);
    },
    onError: () => toast.error("Failed to create run."),
  });

  const valid = name && a && b && a !== b && from && to && from <= to;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New reconciliation run</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="run-name">Name</Label>
            <Input
              id="run-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-a">Source A</Label>
              <Select value={a} onValueChange={(v) => setA(v ?? "")}>
                <SelectTrigger id="run-a">
                  <SelectValue placeholder="Select" />
                </SelectTrigger>
                <SelectContent>
                  {sources?.map((s) => (
                    <SelectItem key={s.id} value={s.id}>
                      {s.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-b">Source B</Label>
              <Select value={b} onValueChange={(v) => setB(v ?? "")}>
                <SelectTrigger id="run-b">
                  <SelectValue placeholder="Select" />
                </SelectTrigger>
                <SelectContent>
                  {sources?.map((s) => (
                    <SelectItem key={s.id} value={s.id}>
                      {s.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-from">From</Label>
              <Input
                id="run-from"
                type="date"
                value={from}
                onChange={(e) => setFrom(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-to">To</Label>
              <Input
                id="run-to"
                type="date"
                value={to}
                onChange={(e) => setTo(e.target.value)}
              />
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!valid || mutation.isPending}
          >
            {mutation.isPending ? "Running…" : "Create run"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
