import { Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { type ReactNode, useRef } from "react";
import { ActivityBar } from "./ActivityBar";
import { BottomPanel } from "./BottomPanel";
import { ConnectionBanner } from "./ConnectionBanner";
import s from "./Frame.module.css";
import { SectionBoundary } from "./SectionBoundary";
import { StatusBar } from "./StatusBar";
import { TitleBar } from "./TitleBar";
import { useAutoCollapse } from "./useAutoCollapse";
export function Frame({ project, onOpenPalette }: { project: ReactNode; onOpenPalette: () => void }) {
  const path = useRouterState({ select: (state) => state.matches.at(-1)?.pathname ?? state.location.pathname });
  const navigate = useNavigate();
  const frame = useRef<HTMLDivElement>(null);
  useAutoCollapse(frame);
  return (
    <div ref={frame} className={s.frame}>
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
        <BottomPanel />
      </main>
      <div className={s.status}>
        <StatusBar />
      </div>
    </div>
  );
}
