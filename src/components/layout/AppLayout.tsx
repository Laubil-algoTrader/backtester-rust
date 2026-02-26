import type { ReactNode } from "react";
import { TopNav } from "./TopNav";

interface AppLayoutProps {
  children: ReactNode;
}

export function AppLayout({ children }: AppLayoutProps) {
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <TopNav />
      <main className="flex-1 overflow-auto px-6 py-5">{children}</main>
    </div>
  );
}
