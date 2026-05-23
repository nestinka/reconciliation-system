/**
 * Format an ISO timestamp/date string for display.
 *
 * Reconciliation dates are operationally meaningful and stored as UTC, so we
 * format in UTC — the displayed day must match the stored value regardless of
 * the viewer's timezone. Returns an em dash for missing values.
 */
export function formatDate(iso?: string | null): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "2-digit",
    month: "short",
    year: "numeric",
    timeZone: "UTC",
  });
}

/** Like {@link formatDate} but includes the time of day (UTC, 24-hour). */
export function formatDateTime(iso?: string | null): string {
  if (!iso) return "—";
  return new Date(iso).toLocaleString("en-GB", {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
    hour12: false,
  });
}
