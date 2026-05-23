import { PageHeader } from "@/components/app/page-header";

export default function DashboardPage() {
  return (
    <>
      <PageHeader
        title="Dashboard"
        description="Overview of reconciliation health and recent activity."
      />
      <p className="text-sm text-muted-foreground">
        Dashboard metrics and charts will appear here.
      </p>
    </>
  );
}
