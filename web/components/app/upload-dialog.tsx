"use client";
import { useState, useEffect } from "react";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
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
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { IngestError } from "@/lib/api/client";
import type {
  SourceListItem,
  IngestFormat,
  CsvMapping,
  AmountMapping,
} from "@/lib/api/client";

export function UploadDialog({
  source,
  open,
  onOpenChange,
}: {
  source: SourceListItem;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const [format, setFormat] = useState<IngestFormat>("csv");
  const [dialectOverride, setDialectOverride] = useState<string>("");
  const [pdfProfileOverride, setPdfProfileOverride] = useState<string>("");
  const [file, setFile] = useState<File | null>(null);
  const [report, setReport] = useState<{
    kind: "parse" | "duplicate";
    rows?: { row: number; field: string; message: string }[];
    refs?: string[];
  } | null>(null);

  /* eslint-disable react-hooks/set-state-in-effect */
  useEffect(() => {
    if (!open) {
      setFormat("csv");
    }
    setFile(null);
    setReport(null);
  }, [open]);

  useEffect(() => {
    setFile(null);
    setReport(null);
  }, [format]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const { data: pdfProfiles = [] } = useQuery({
    queryKey: ["pdf-profiles", tenantId],
    queryFn: () => api.listPdfProfiles(tenantId),
    enabled: format === "pdf",
  });

  // CSV mapping fields (indices, 0-based)
  const [hasHeader, setHasHeader] = useState(true);
  const [delimiter, setDelimiter] = useState(44);
  const [dateFormat, setDateFormat] = useState("%Y-%m-%d");
  const [refCol, setRefCol] = useState(0);
  const [dateCol, setDateCol] = useState(1);
  const [descCol, setDescCol] = useState(4);
  const [amountKind, setAmountKind] = useState<"signed" | "debitCredit">("signed");
  const [amountCol, setAmountCol] = useState(2);
  const [debitWhenNegative, setDebitWhenNegative] = useState(true);
  const [debitCol, setDebitCol] = useState(2);
  const [creditCol, setCreditCol] = useState(3);

  const buildMapping = (): CsvMapping => {
    const amount: AmountMapping =
      amountKind === "signed"
        ? { signed: { column: { index: amountCol }, debitWhenNegative } }
        : {
            debitCredit: {
              debit: { index: debitCol },
              credit: { index: creditCol },
            },
          };
    return {
      hasHeader,
      delimiter,
      externalRef: { index: refCol },
      valueDate: { index: dateCol },
      dateFormat,
      amount,
      description: { index: descCol },
    };
  };

  const mutation = useMutation({
    mutationFn: () => {
      if (!file) throw new Error("No file selected");
      return api.ingestFile(
        tenantId,
        source.id,
        format,
        file,
        format === "csv" ? buildMapping() : undefined,
        dialectOverride || undefined,
        pdfProfileOverride || undefined,
      );
    },
    onSuccess: (res) => {
      setReport(null);
      void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] });
      toast.success(
        `${res.ingested} transaction${res.ingested === 1 ? "" : "s"} ingested.`
      );
      onOpenChange(false);
    },
    onError: (e) => {
      if (e instanceof IngestError)
        setReport({ kind: e.code, rows: e.rows, refs: e.refs });
      else toast.error("Ingestion failed.");
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Upload to {source.name}</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-format">Format</Label>
            <Select
              value={format}
              onValueChange={(v) => setFormat(v as IngestFormat)}
            >
              <SelectTrigger id="up-format">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">Auto-detect</SelectItem>
                <SelectItem value="csv">CSV (with column mapping)</SelectItem>
                <SelectItem value="camt053">CAMT.053 (ISO 20022 XML)</SelectItem>
                <SelectItem value="mt940">MT940 (SWIFT statement)</SelectItem>
                <SelectItem value="mt942">MT942 (intra-day)</SelectItem>
                <SelectItem value="bai2">BAI v2 (US bank file)</SelectItem>
                <SelectItem value="pdf">PDF statement</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {format === "auto" && (
            <p className="text-sm text-muted-foreground">
              The format is detected from the file. CSV files must be uploaded with the explicit CSV format.
            </p>
          )}

          {(format === "mt940" || format === "mt942") && !source.formatDialect && (
            <p className="text-sm text-amber-600 dark:text-amber-400">
              This source has no MT940/MT942 dialect set. Using{" "}
              <strong>Generic</strong>. Edit the source to choose a dialect.
            </p>
          )}

          {(format === "mt940" || format === "mt942") && (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="up-dialect-ovr">Dialect (override for this upload)</Label>
              <Select value={dialectOverride || "__default__"} onValueChange={(v) => setDialectOverride(v === null || v === "__default__" ? "" : v)}>
                <SelectTrigger id="up-dialect-ovr"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="__default__">Use source default{source.formatDialect ? ` (${source.formatDialect})` : ""}</SelectItem>
                  <SelectItem value="generic">Generic</SelectItem>
                  <SelectItem value="subfielded">Subfielded (DE/NL/BE)</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}

          {format === "pdf" && !source.pdfProfile && !pdfProfileOverride && (
            <p className="text-sm text-amber-600 dark:text-amber-400">
              This source has no PDF profile set. Edit the source to choose one
              before uploading a PDF.
            </p>
          )}
          {format === "pdf" && source.pdfProfile && (
            <p className="text-sm text-muted-foreground">
              Using PDF profile <strong>{source.pdfProfile}</strong>.
            </p>
          )}

          {format === "pdf" && (
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="up-pdf-ovr">PDF profile (override for this upload)</Label>
              <Select value={pdfProfileOverride || "__default__"} onValueChange={(v) => setPdfProfileOverride(v === null || v === "__default__" ? "" : v)}>
                <SelectTrigger id="up-pdf-ovr"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="__default__">Use source default{source.pdfProfile ? ` (${source.pdfProfile})` : ""}</SelectItem>
                  {pdfProfiles.map((p) => (<SelectItem key={p} value={p}>{p}</SelectItem>))}
                </SelectContent>
              </Select>
            </div>
          )}

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-file">File</Label>
            <Input
              id="up-file"
              type="file"
              onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              accept={fileAccept(format)}
            />
          </div>

          {format === "csv" && (
            <div className="flex flex-col gap-3 rounded-md border border-border p-3">
              <p className="text-xs text-muted-foreground">
                Map CSV columns (0-based index).
              </p>
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={hasHeader}
                  onCheckedChange={(c) => setHasHeader(!!c)}
                />
                Has header row
              </label>
              <div className="grid grid-cols-2 gap-3">
                <NumberField
                  label="Reference col"
                  value={refCol}
                  onChange={setRefCol}
                  id="m-ref"
                />
                <NumberField
                  label="Date col"
                  value={dateCol}
                  onChange={setDateCol}
                  id="m-date"
                />
                <NumberField
                  label="Description col"
                  value={descCol}
                  onChange={setDescCol}
                  id="m-desc"
                />
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="m-delim">Delimiter</Label>
                  <Select
                    value={String(delimiter)}
                    onValueChange={(v) => setDelimiter(Number(v))}
                  >
                    <SelectTrigger id="m-delim">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="44">Comma</SelectItem>
                      <SelectItem value="59">Semicolon</SelectItem>
                      <SelectItem value="9">Tab</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="m-dfmt">Date format</Label>
                <Input
                  id="m-dfmt"
                  value={dateFormat}
                  onChange={(e) => setDateFormat(e.target.value)}
                  placeholder="%Y-%m-%d"
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="m-amtkind">Amount encoding</Label>
                <Select
                  value={amountKind}
                  onValueChange={(v) =>
                    setAmountKind(v as "signed" | "debitCredit")
                  }
                >
                  <SelectTrigger id="m-amtkind">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="signed">
                      Single signed column
                    </SelectItem>
                    <SelectItem value="debitCredit">
                      Separate debit/credit columns
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              {amountKind === "signed" ? (
                <div className="grid grid-cols-2 gap-3">
                  <NumberField
                    label="Amount col"
                    value={amountCol}
                    onChange={setAmountCol}
                    id="m-amt"
                  />
                  <label className="flex items-center gap-2 text-sm mt-6">
                    <Checkbox
                      checked={debitWhenNegative}
                      onCheckedChange={(c) => setDebitWhenNegative(!!c)}
                    />
                    Negative = debit
                  </label>
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-3">
                  <NumberField
                    label="Debit col"
                    value={debitCol}
                    onChange={setDebitCol}
                    id="m-debit"
                  />
                  <NumberField
                    label="Credit col"
                    value={creditCol}
                    onChange={setCreditCol}
                    id="m-credit"
                  />
                </div>
              )}
            </div>
          )}

          {report && (
            <div
              role="alert"
              className="rounded-md border border-danger/30 bg-danger/5 p-3 text-sm text-danger max-h-40 overflow-auto"
            >
              {report.kind === "parse" ? (
                <>
                  <p className="font-medium mb-1">
                    File rejected — fix these rows:
                  </p>
                  <ul className="list-disc pl-4">
                    {report.rows?.map((r) => (
                      <li key={r.row}>
                        Row {r.row}: {r.field} — {r.message}
                      </li>
                    ))}
                  </ul>
                </>
              ) : (
                <>
                  <p className="font-medium mb-1">
                    Duplicate references already loaded:
                  </p>
                  <p>{report.refs?.join(", ")}</p>
                </>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!file || mutation.isPending || (format === "pdf" && !source.pdfProfile && !pdfProfileOverride)}
          >
            {mutation.isPending ? "Uploading…" : "Upload"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function fileAccept(format: IngestFormat): string {
  switch (format) {
    case "csv":
      return ".csv,text/csv";
    case "camt053":
      return ".xml,text/xml,application/xml";
    case "mt940":
      return ".mt940,.sta,.txt,text/plain";
    case "mt942":
      return ".mt942,.sta,.txt,text/plain";
    case "bai2":
      return ".bai,.bai2,.txt,text/plain";
    case "pdf":
      return ".pdf,application/pdf";
    case "auto":
      return "";
  }
}

function NumberField({
  label,
  value,
  onChange,
  id,
}: {
  label: string;
  value: number;
  onChange: (n: number) => void;
  id: string;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        type="number"
        min={0}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </div>
  );
}
