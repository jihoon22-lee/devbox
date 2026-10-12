import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { ActivityBar } from "./ActivityBar";
import { ConnectionBanner } from "./ConnectionBanner";
import s from "./Frame.module.css";
import { SectionBoundary } from "./SectionBoundary";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
export function Frame({ project, onOpenPalette }: { project: ReactNode; onOpenPalette: () => void }) {
  const path = useRouterState({ select: (state) => state.matches.at(-1)?.pathname ?? state.location.pathname });
  const navigate = useNavigate();
  return (
    <div className={s.frame}>
      <div className={s.title}>
        <TitleBar project={project} onOpenPalette={onOpenPalette} />
      </div>
      <div className={s.activity}>
        <ActivityBar />
      </div>
      <main className={s.main}>
        <ConnectionBanner />
        <div className={s.content}>
          <SectionBoundary
            resetKey={path}
            onNavigate={() => void navigate({ to: path === "/agents" ? "/settings" : "/agents" })}
          >
            <Outlet />
          </SectionBoundary>
        </div>
      </main>
      <div className={s.status}>
        <StatusBar />
      </div>
    </div>
  );
}
