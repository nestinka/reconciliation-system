import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";

export function useTenants() {
  const api = useApi();
  return useQuery({
    queryKey: ["tenants"],
    queryFn: () => api.listTenants(),
  });
}

export function useUsers() {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["users", tenantId],
    queryFn: () => api.listUsers(tenantId),
  });
}

export function useMembers() {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["members", tenantId],
    queryFn: () => api.listMembers(tenantId),
  });
}
