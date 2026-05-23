import { PageHeader } from "@/components/app/page-header";

export default function RunsPage() {
  return (
    <>
      <PageHeader
        title="Reconciliation Runs"
        description="Browse and inspect reconciliation run history."
      />
      <p className="text-sm text-muted-foreground">
        Run list and detail views will appear here.
      </p>
    </>
  );
}
