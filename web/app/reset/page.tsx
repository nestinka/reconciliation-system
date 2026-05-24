"use client";

import { Suspense, useState } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { resetRequest, AuthError } from "@/lib/auth/api";

const resetSchema = z
  .object({
    newPassword: z.string().min(8, "Password must be at least 8 characters"),
    confirmPassword: z.string().min(1, "Please confirm your password"),
  })
  .refine((d) => d.newPassword === d.confirmPassword, {
    message: "Passwords do not match",
    path: ["confirmPassword"],
  });

type ResetFormValues = z.infer<typeof resetSchema>;

function ResetPageInner() {
  const searchParams = useSearchParams();
  const token = searchParams.get("token") ?? "";
  const router = useRouter();
  const [tokenError, setTokenError] = useState(false);

  const {
    register,
    handleSubmit,
    formState: { errors, isSubmitting },
  } = useForm<ResetFormValues>({
    resolver: zodResolver(resetSchema),
  });

  if (!token) {
    return (
      <div className="text-center">
        <p className="text-sm text-destructive">
          This reset link is invalid or expired.
        </p>
        <Link
          href="/forgot"
          className="mt-4 inline-block text-sm text-foreground underline underline-offset-4 hover:text-muted-foreground"
        >
          Request a new reset link
        </Link>
      </div>
    );
  }

  if (tokenError) {
    return (
      <div className="text-center">
        <p className="text-sm text-destructive">
          This reset link is invalid or expired.
        </p>
        <Link
          href="/forgot"
          className="mt-4 inline-block text-sm text-foreground underline underline-offset-4 hover:text-muted-foreground"
        >
          Request a new reset link
        </Link>
      </div>
    );
  }

  async function onSubmit(values: ResetFormValues) {
    try {
      await resetRequest(token, values.newPassword);
      toast.success("Password reset successfully. You can now sign in.");
      router.replace("/login");
    } catch (err) {
      if (err instanceof AuthError && (err.status === 400 || err.status === 404)) {
        setTokenError(true);
      } else {
        toast.error("Failed to reset password. Please try again.");
      }
    }
  }

  return (
    <form
      onSubmit={handleSubmit(onSubmit)}
      className="flex flex-col gap-4"
      noValidate
    >
      <div className="flex flex-col gap-1.5">
        <label
          htmlFor="newPassword"
          className="text-sm font-medium text-foreground"
        >
          New password
        </label>
        <Input
          id="newPassword"
          type="password"
          autoComplete="new-password"
          placeholder="Min 8 characters"
          aria-invalid={!!errors.newPassword}
          aria-describedby={errors.newPassword ? "new-pw-error" : undefined}
          {...register("newPassword")}
        />
        {errors.newPassword && (
          <p id="new-pw-error" className="text-xs text-destructive" role="alert">
            {errors.newPassword.message}
          </p>
        )}
      </div>

      <div className="flex flex-col gap-1.5">
        <label
          htmlFor="confirmPassword"
          className="text-sm font-medium text-foreground"
        >
          Confirm password
        </label>
        <Input
          id="confirmPassword"
          type="password"
          autoComplete="new-password"
          placeholder="Repeat new password"
          aria-invalid={!!errors.confirmPassword}
          aria-describedby={errors.confirmPassword ? "confirm-pw-error" : undefined}
          {...register("confirmPassword")}
        />
        {errors.confirmPassword && (
          <p id="confirm-pw-error" className="text-xs text-destructive" role="alert">
            {errors.confirmPassword.message}
          </p>
        )}
      </div>

      <Button type="submit" className="w-full" disabled={isSubmitting}>
        {isSubmitting ? "Resetting…" : "Reset password"}
      </Button>

      <p className="text-center text-sm text-muted-foreground">
        Remember your password?{" "}
        <Link
          href="/login"
          className="text-foreground underline underline-offset-4 hover:text-muted-foreground"
        >
          Sign in
        </Link>
      </p>
    </form>
  );
}

function ResetPageSkeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="h-9 w-full animate-pulse rounded bg-muted" />
      <div className="h-9 w-full animate-pulse rounded bg-muted" />
      <div className="h-9 w-full animate-pulse rounded bg-muted" />
    </div>
  );
}

export default function ResetPage() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm">
        <div className="mb-8 text-center">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            Reset password
          </h1>
          <p className="mt-2 text-sm text-muted-foreground">
            Enter your new password below.
          </p>
        </div>
        <Suspense fallback={<ResetPageSkeleton />}>
          <ResetPageInner />
        </Suspense>
      </div>
    </div>
  );
}
