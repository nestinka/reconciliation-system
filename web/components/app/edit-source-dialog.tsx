"use client";

import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { FormatDialect } from "@/lib/api/client";
import type { Source } from "@/lib/domain/types";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const DIALECT_NONE = "__none__";

const schema = z.object({
  name: z.string().min(1, "Name is required").max(80, "Name too long"),
  formatDialect: z.enum(["generic", "subfielded"]).nullable(),
  pdfProfile: z.string().nullable(),
});
type FormValues = z.infer<typeof schema>;

interface Props {
  source: Source;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function EditSourceDialog({ source, open, onOpenChange }: Props) {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const {
    register,
    handleSubmit,
    reset,
    setValue,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: source.name,
      formatDialect: source.formatDialect ?? null,
      pdfProfile: source.pdfProfile ?? null,
    },
  });
  const formatDialect = watch("formatDialect");
  const pdfProfile = watch("pdfProfile");

  const { data: pdfProfiles = [] } = useQuery({
    queryKey: ["pdf-profiles", tenantId],
    queryFn: () => api.listPdfProfiles(tenantId),
  });

  // Re-seed when opening for a different source (or after a save).
  useEffect(() => {
    if (open) {
      reset({
        name: source.name,
        formatDialect: source.formatDialect ?? null,
        pdfProfile: source.pdfProfile ?? null,
      });
    }
  }, [open, source.id, source.name, source.formatDialect, source.pdfProfile, reset]);

  const updateMutation = useMutation({
    mutationFn: (values: FormValues) => {
      const patch: { name?: string; formatDialect?: FormatDialect | null; pdfProfile?: string | null } = {};
      if (values.name !== source.name) patch.name = values.name;
      if (values.formatDialect !== source.formatDialect) {
        patch.formatDialect = values.formatDialect;
      }
      if (values.pdfProfile !== (source.pdfProfile ?? null)) {
        patch.pdfProfile = values.pdfProfile;
      }
      return api.updateSource(tenantId, source.id, patch);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] });
      toast.success("Source updated.");
      onOpenChange(false);
    },
    onError: () => toast.error("Failed to update source."),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit source</DialogTitle>
        </DialogHeader>
        <form
          onSubmit={handleSubmit((v) => updateMutation.mutate(v))}
          className="flex flex-col gap-4"
          noValidate
        >
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-src-name">Name</Label>
            <Input
              id="edit-src-name"
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
            <Label htmlFor="edit-src-dialect">
              MT940 / MT942 dialect{" "}
              <span className="text-muted-foreground font-normal">
                (optional)
              </span>
            </Label>
            <Select
              value={formatDialect ?? DIALECT_NONE}
              onValueChange={(v) =>
                setValue(
                  "formatDialect",
                  v === DIALECT_NONE ? null : (v as FormatDialect),
                )
              }
            >
              <SelectTrigger id="edit-src-dialect" aria-label="MT940 dialect">
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
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="edit-src-pdf-profile">
              PDF profile{" "}
              <span className="text-muted-foreground font-normal">
                (optional)
              </span>
            </Label>
            <Select
              value={pdfProfile ?? DIALECT_NONE}
              onValueChange={(v) =>
                setValue("pdfProfile", v === DIALECT_NONE ? null : v)
              }
            >
              <SelectTrigger id="edit-src-pdf-profile" aria-label="PDF profile">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={DIALECT_NONE}>Not applicable</SelectItem>
                {pdfProfiles.map((p) => (
                  <SelectItem key={p} value={p}>{p}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isSubmitting || updateMutation.isPending}>
              {updateMutation.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
