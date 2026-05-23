import { PageHeader } from "@/components/app/page-header";

export default function ExceptionsPage() {
  return (
    <>
      <PageHeader
        title="Exceptions"
        description="Open breaks and exceptions requiring investigation or approval."
      />
      <p className="text-sm text-muted-foreground">
        Exception list and case detail views will appear here.
      </p>
    </>
  );
}
