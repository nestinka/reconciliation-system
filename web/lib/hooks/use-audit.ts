import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { AuditQuery } from "@/lib/api/client";

export function useAudit(q?: AuditQuery) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["audit", tenantId, q],
    queryFn: () => api.listAudit(tenantId, q),
  });
}

export function useAnchors(limit = 50) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["audit", "anchors", tenantId, limit],
    queryFn: () => api.listAnchors(tenantId, limit),
  });
}

export function useControls() {
  const api = useApi();
  return useQuery({
    queryKey: ["audit", "controls"],
    queryFn: () => api.listControls(),
  });
}
