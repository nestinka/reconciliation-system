"use client";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { PlusCircle, Upload as UploadIcon, Database } from "lucide-react";
import { PageHeader } from "@/components/app/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useSources } from "@/lib/hooks/use-sources";
import { UploadDialog } from "@/components/app/upload-dialog";
import { EditSourceDialog } from "@/components/app/edit-source-dialog";
import type { SourceListItem, FormatDialect } from "@/lib/api/client";
import type { SourceKind } from "@/lib/domain/types";

const KIND_OPTIONS: { value: SourceKind; label: string }[] = [
  { value: "bank", label: "Bank" },
  { value: "ledger", label: "Ledger" },
  { value: "cross_system", label: "Cross-system" },
];

// MT940 dialect select uses an empty-string sentinel to mean "not applicable"
// (null on the wire). Base UI's Select can't bind a real null value.
const DIALECT_NONE = "__none__";

const schema = z.object({
  name: z.string().min(1, "Name is required"),
  kind: z.enum(["bank", "ledger", "cross_system"]),
  currency: z.string().min(3, "3-letter currency code").max(3, "3-letter currency code"),
  // Optional MT940 dialect — null for non-MT940 sources, "generic" or
  // "subfielded" when the source receives MT940 statements.
  formatDialect: z.enum(["generic", "subfielded"]).nullable(),
});

type FormValues = z.infer<typeof schema>;

export default function SourcesPage() {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const { data: sources, isLoading } = useSources();
  const [showNew, setShowNew] = useState(false);
  const [uploadTarget, setUploadTarget] = useState<SourceListItem | null>(null);
  const [editTarget, setEditTarget] = useState<SourceListItem | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: FormValues) => api.createSource(tenantId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] });
      toast.success("Source created.");
      setShowNew(false);
      reset();
    },
    onError: () => toast.error("Failed to create source."),
  });

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    watch,
    formState: { errors },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { kind: "bank", currency: "GBP", formatDialect: null },
  });

  const kind = watch("kind");
  const formatDialect = watch("formatDialect");

  // Reset form state whenever the dialog is closed so a stale entry doesn't
  // bleed into the next open (mirrors the UploadDialog reset-on-close pattern).
  useEffect(() => {
    if (!showNew) {
      reset({ name: "", kind: "bank", currency: "GBP", formatDialect: null });
    }
  }, [showNew, reset]);

  return (
    <>
      <div className="flex flex-col gap-6">
        <div className="flex items-center justify-between">
          <PageHeader
            title="Sources"
            description="Manage data sources and ingest bank/ledger files."
          />
          <Button onClick={() => setShowNew(true)} className="gap-2">
            <PlusCircle aria-hidden className="size-4" />
            New source
          </Button>
        </div>

        {isLoading ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 3 }).map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </div>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Kind</TableHead>
                  <TableHead>Currency</TableHead>
                  <TableHead className="text-right">Transactions</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {sources?.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell className="font-medium">
                      <div className="flex items-center gap-2">
                        <span>{s.name}</span>
                        {s.formatDialect && (
                          <Badge variant="secondary" className="text-xs">
                            MT940 ·{" "}
                            {s.formatDialect === "subfielded"
                              ? "Subfielded"
                              : "Generic"}
                          </Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="capitalize text-muted-foreground">
                      {s.kind.replace("_", " ")}
                    </TableCell>
                    <TableCell>{s.currency}</TableCell>
                    <TableCell className="text-right tabular-nums">
                      {s.txnCount}
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        variant="outline"
                        size="sm"
                        className="gap-1.5 mr-1.5"
                        onClick={() => setEditTarget(s)}
                      >
                        Edit
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        className="gap-1.5"
                        onClick={() => setUploadTarget(s)}
                      >
                        <UploadIcon aria-hidden className="size-3.5" />
                        Upload
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
                {sources?.length === 0 && (
                  <TableRow>
                    <TableCell
                      colSpan={5}
                      className="text-center text-muted-foreground py-8"
                    >
                      <Database
                        aria-hidden
                        className="size-5 mx-auto mb-2 opacity-50"
                      />
                      No sources yet. Create one to start ingesting.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        )}
      </div>

      <Dialog open={showNew} onOpenChange={setShowNew}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New source</DialogTitle>
          </DialogHeader>
          <form
            onSubmit={handleSubmit((v) => createMutation.mutate(v))}
            className="flex flex-col gap-4"
            noValidate
          >
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-name">Name</Label>
              <Input
                id="src-name"
                {...register("name")}
                aria-invalid={!!errors.name}
              />
              {errors.name && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.name.message}
                </p>
              )}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-kind">Kind</Label>
              <Select
                value={kind}
                onValueChange={(v) => setValue("kind", (v ?? "bank") as SourceKind)}
              >
                <SelectTrigger id="src-kind">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KIND_OPTIONS.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-ccy">Currency</Label>
              <Input
                id="src-ccy"
                {...register("currency")}
                aria-invalid={!!errors.currency}
              />
              {errors.currency && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.currency.message}
                </p>
              )}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-dialect">
                MT940 dialect{" "}
                <span className="text-muted-foreground font-normal">
                  (optional)
                </span>
              </Label>
              <Select
                value={formatDialect ?? DIALECT_NONE}
                onValueChange={(v) =>
                  setValue(
                    "formatDialect",
                    v === DIALECT_NONE ? null : (v as FormatDialect)
                  )
                }
              >
                <SelectTrigger id="src-dialect" aria-label="MT940 dialect">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={DIALECT_NONE}>Not applicable</SelectItem>
                  <SelectItem value="generic">Generic</SelectItem>
                  <SelectItem value="subfielded">
                    Subfielded (DE/NL/BE)
                  </SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                Set this only if this source will receive MT940 statements. For
                Deutsche Bank, ING, ABN AMRO, Rabobank, and most other European
                banks, choose Subfielded.
              </p>
            </div>
            <DialogFooter>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? "Creating…" : "Create source"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {uploadTarget && (
        <UploadDialog
          source={uploadTarget}
          open={!!uploadTarget}
          onOpenChange={(o: boolean) => !o && setUploadTarget(null)}
        />
      )}

      {editTarget && (
        <EditSourceDialog
          source={editTarget}
          open={!!editTarget}
          onOpenChange={(o) => !o && setEditTarget(null)}
        />
      )}
    </>
  );
}
