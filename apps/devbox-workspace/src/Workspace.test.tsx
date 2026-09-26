import { cleanup, render, screen } from "@testing-library/react";
import { StrictMode, useEffect } from "react";
import { renderToString } from "react-dom/server";
import { afterEach, expect, it, vi } from "vitest";
import type { ShellContentProps } from "@devbox/product-shell";
import { fixtureDescription } from "@devbox/product-shell/api";
import { productInstallationId } from "@devbox/workspace-features/transport";

const observed = vi.hoisted(() => ({ configure: vi.fn(), mount: vi.fn() }));
vi.mock("@devbox/product-shell", () => ({
  ProductShell: ({ renderContent }: { renderContent: (props: ShellContentProps) => React.ReactNode }) =>
    renderContent({ description, route: "overview", refreshContext: async () => {}, navigate: () => {} }),
}));
vi.mock("@devbox/product-shell/api", async (original) => ({
  ...(await original<typeof import("@devbox/product-shell/api")>()),
  nativeMode: true,
  productDataAvailable: () => true,
}));
vi.mock("@devbox/workspace-features/transport", async (original) => {
  const actual = await original<typeof import("@devbox/workspace-features/transport")>();
  return {
    ...actual,
    configureProductTransport: (...args: Parameters<typeof actual.configureProductTransport>) => {
      observed.configure(...args);
      actual.configureProductTransport(...args);
    },
  };
});
vi.mock("./RegistryGate", () => ({
  default: ({ onReady }: { onReady: () => void }) => {
    observed.mount(productInstallationId());
    useEffect(onReady, [onReady]);
    return <p>native registry ready</p>;
  },
}));
vi.mock("./TerminalLogBridge", () => ({ default: () => null }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
import Workspace from "./Workspace";

const description = fixtureDescription("workspace");
afterEach(cleanup);
it("binds in an effect before native features mount and survives StrictMode replay", async () => {
  renderToString(<Workspace />);
  expect(observed.configure).not.toHaveBeenCalled();
  render(
    <StrictMode>
      <Workspace />
    </StrictMode>,
  );
  await screen.findByText("native registry ready");
  expect(observed.configure).toHaveBeenCalled();
  expect(observed.mount).toHaveBeenCalledWith(description.handshake.installationId);
});
