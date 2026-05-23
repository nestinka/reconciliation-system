import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { NuqsAdapter } from "nuqs/adapters/next/app";
import { ThemeProvider } from "@/lib/providers/theme-provider";
import { QueryProvider } from "@/lib/providers/query-provider";
import { ApiProvider } from "@/lib/api/provider";
import { TenantProvider } from "@/lib/providers/tenant-provider";
import { CurrentUserProvider } from "@/lib/providers/current-user-provider";
import { Toaster } from "@/components/ui/sonner";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Reconciliation Platform",
  description: "Financial reconciliation and break management",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="min-h-full flex flex-col">
        <ThemeProvider>
          <QueryProvider>
            <ApiProvider>
              <TenantProvider>
                <CurrentUserProvider>
                  <NuqsAdapter>
                    {children}
                    <Toaster />
                  </NuqsAdapter>
                </CurrentUserProvider>
              </TenantProvider>
            </ApiProvider>
          </QueryProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
