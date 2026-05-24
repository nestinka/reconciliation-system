"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { PlusCircle, Trash2, UserCog } from "lucide-react";

import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
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
import { useAuth } from "@/lib/auth/auth-provider";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { UserRole } from "@/lib/domain/types";

const ROLE_OPTIONS: { value: UserRole; label: string }[] = [
  { value: "operator", label: "Operator" },
  { value: "approver", label: "Approver" },
  { value: "admin", label: "Admin" },
];

const addUserSchema = z.object({
  name: z.string().min(1, "Name is required"),
  email: z.string().email("Enter a valid email"),
  role: z.enum(["operator", "approver", "admin"]),
  password: z.string().min(8, "Password must be at least 8 characters"),
});

type AddUserFormValues = z.infer<typeof addUserSchema>;

export default function UsersPage() {
  const { user } = useAuth();
  const router = useRouter();
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();

  const [showAddUser, setShowAddUser] = useState(false);

  // Guard: only admins can access this page
  useEffect(() => {
    if (user && user.role !== "admin") {
      router.replace("/dashboard");
    }
  }, [user, router]);

  const { data: users, isLoading } = useQuery({
    queryKey: ["users", tenantId],
    queryFn: () => api.listUsers(tenantId),
    enabled: user?.role === "admin",
  });

  const createUserMutation = useMutation({
    mutationFn: (input: AddUserFormValues) =>
      api.createUser(tenantId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["users", tenantId] });
      toast.success("User created successfully.");
      setShowAddUser(false);
      reset();
    },
    onError: () => {
      toast.error("Failed to create user.");
    },
  });

  const updateUserMutation = useMutation({
    mutationFn: ({
      userId,
      patch,
    }: {
      userId: string;
      patch: { role?: UserRole; disabled?: boolean };
    }) => api.updateUser(tenantId, userId, patch),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["users", tenantId] });
      toast.success("User updated.");
    },
    onError: () => {
      toast.error("Failed to update user.");
    },
  });

  const deleteUserMutation = useMutation({
    mutationFn: (userId: string) => api.deleteUser(tenantId, userId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["users", tenantId] });
      toast.success("User removed.");
    },
    onError: () => {
      toast.error("Failed to remove user.");
    },
  });

  const {
    register,
    handleSubmit,
    reset,
    setValue,
    watch,
    formState: { errors, isSubmitting },
  } = useForm<AddUserFormValues>({
    resolver: zodResolver(addUserSchema),
    defaultValues: { role: "operator" },
  });

  const selectedRole = watch("role");

  if (user?.role !== "admin") {
    return null;
  }

  return (
    <>
      <div className="flex flex-col gap-6">
        <div className="flex items-center justify-between">
          <PageHeader
            title="Users"
            description="Manage team members and their roles."
          />
          <Button onClick={() => setShowAddUser(true)} className="gap-2">
            <PlusCircle aria-hidden className="size-4" />
            Add user
          </Button>
        </div>

        {isLoading ? (
          <div className="flex flex-col gap-2">
            {Array.from({ length: 4 }).map((_, i) => (
              <Skeleton key={i} className="h-12 w-full" />
            ))}
          </div>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Email</TableHead>
                  <TableHead>Role</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {users?.map((u) => (
                  <TableRow key={u.id}>
                    <TableCell className="font-medium">{u.name}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {u.email ?? "—"}
                    </TableCell>
                    <TableCell>
                      <Select
                        value={u.role}
                        onValueChange={(val) =>
                          updateUserMutation.mutate({
                            userId: u.id,
                            patch: { role: val as UserRole },
                          })
                        }
                      >
                        <SelectTrigger
                          size="sm"
                          className="w-32"
                          aria-label={`Role for ${u.name}`}
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {ROLE_OPTIONS.map((opt) => (
                            <SelectItem key={opt.value} value={opt.value}>
                              {opt.label}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={u.disabled ? "destructive" : "default"}
                        className="cursor-pointer select-none"
                        onClick={() =>
                          updateUserMutation.mutate({
                            userId: u.id,
                            patch: { disabled: !u.disabled },
                          })
                        }
                      >
                        {u.disabled ? "Disabled" : "Active"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right">
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Remove ${u.name}`}
                        onClick={() => deleteUserMutation.mutate(u.id)}
                      >
                        <Trash2 aria-hidden className="size-4 text-destructive" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
                {users?.length === 0 && (
                  <TableRow>
                    <TableCell colSpan={5} className="text-center text-muted-foreground py-8">
                      No users found.
                    </TableCell>
                  </TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        )}
      </div>

      <Dialog open={showAddUser} onOpenChange={setShowAddUser}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              <span className="flex items-center gap-2">
                <UserCog aria-hidden className="size-5" />
                Add user
              </span>
            </DialogTitle>
          </DialogHeader>
          <form
            onSubmit={handleSubmit((values) => createUserMutation.mutate(values))}
            className="flex flex-col gap-4"
            noValidate
          >
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-name">Name</Label>
              <Input
                id="add-name"
                placeholder="Full name"
                aria-invalid={!!errors.name}
                {...register("name")}
              />
              {errors.name && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.name.message}
                </p>
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-email">Email</Label>
              <Input
                id="add-email"
                type="email"
                placeholder="user@example.com"
                aria-invalid={!!errors.email}
                {...register("email")}
              />
              {errors.email && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.email.message}
                </p>
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-role">Role</Label>
              <Select
                value={selectedRole}
                onValueChange={(val) => setValue("role", val as UserRole)}
              >
                <SelectTrigger id="add-role" aria-invalid={!!errors.role}>
                  <SelectValue placeholder="Select role" />
                </SelectTrigger>
                <SelectContent>
                  {ROLE_OPTIONS.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {errors.role && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.role.message}
                </p>
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="add-password">Temporary password</Label>
              <Input
                id="add-password"
                type="password"
                autoComplete="new-password"
                placeholder="Min 8 characters"
                aria-invalid={!!errors.password}
                {...register("password")}
              />
              {errors.password && (
                <p className="text-xs text-destructive" role="alert">
                  {errors.password.message}
                </p>
              )}
            </div>

            <DialogFooter>
              <Button type="submit" disabled={isSubmitting || createUserMutation.isPending}>
                {createUserMutation.isPending ? "Creating…" : "Create user"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
