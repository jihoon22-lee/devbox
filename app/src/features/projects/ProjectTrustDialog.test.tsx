import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Methods } from "../../rpc/gen/rpc";
import { ProjectTrustDialog } from "./ProjectTrustDialog";
import { trustPreview } from "./trust-fixtures";

test("approval shows logical controls in all repository text without modifying approved bytes", async () => {
  const command = ": \u202e; touch owned-marker # efas ohce\u202c";
  const preview = trustPreview("a", {
    projectName: "repo\u200f",
    canonicalPath: "/fixture/\u202eproject\u202c",
    configPath: "/fixture/\u2066config\u2069",
    executionDirectories: ["/fixture/\u061cwork"],
    config: {
      agent: { copy: [], setup: [command], test: command, teardown: [command] },
      launch: {
        services: [],
        terminals: [{ name: "term\u200e", command }],
        agents: [
          { tool: "tool\u202a", prompt: command, worktree: true },
          { tool: "tool\u202b", prompt: command, worktree: false },
        ],
      },
    },
  }) as Methods["projects.trust_preview"]["result"];
  const original = JSON.stringify(preview);
  const approve = vi.fn();
  const { container } = render(
    <ProjectTrustDialog
      preview={preview}
      pending={false}
      notice=""
      onCancel={vi.fn()}
      onRetry={vi.fn()}
      onApprove={approve}
    />,
  );
  const text = container.ownerDocument.body.textContent ?? "";
  expect(text).not.toMatch(/\p{Bidi_Control}/u);
  for (const escape of [
    "\\u202E",
    "\\u202C",
    "\\u200F",
    "\\u2066",
    "\\u2069",
    "\\u061C",
    "\\u200E",
    "\\u202A",
    "\\u202B",
  ])
    expect(text).toContain(escape);
  const blocks = [...document.querySelectorAll("pre")];
  expect(blocks).toHaveLength(6);
  for (const block of blocks) expect(block.textContent).toBe(": \\u202E; touch owned-marker # efas ohce\\u202C");
  expect(screen.getByRole("button", { name: "승인 취소" })).toHaveFocus();
  await userEvent.click(screen.getByRole("button", { name: "명령 실행 승인" }));
  expect(approve).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(preview)).toBe(original);
});

test("control notation stays distinct from literal escapes and preserves readable Unicode and line breaks", () => {
  const command = "printf '\\u202E'\n\t# עברית العربية\u202e\u000d\u001b\u200b\u{e0001}";
  const preview = trustPreview("a", {
    config: {
      agent: { copy: [], setup: [command], test: null, teardown: [] },
      launch: { services: [], terminals: [], agents: [] },
    },
  }) as Methods["projects.trust_preview"]["result"];
  render(
    <ProjectTrustDialog
      preview={preview}
      pending={false}
      notice=""
      onCancel={vi.fn()}
      onRetry={vi.fn()}
      onApprove={vi.fn()}
    />,
  );
  expect(document.querySelector("pre")?.textContent).toBe(
    "printf '\\\\u202E'\n\t# עברית العربية\\u202E\\u000D\\u001B\\u200B\\uDB40\\uDC01",
  );
  expect(preview.config?.agent.setup[0]).toBe(command);
});
