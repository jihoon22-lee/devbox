import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, expect, it, vi } from "vitest";
import RecoveryBanner from "./RecoveryBanner";
afterEach(cleanup);
const entry = {path:"Notes/a.md",content:"draft",baseRevision:"r1",savedAtMs:Date.UTC(2026,8,23)};
it("lists current notes and routes open/discard accessibly", async()=>{
  const onOpen=vi.fn(), onDiscard=vi.fn();
  const {container}=render(<RecoveryBanner entries={[entry]} otherVaultCount={0} busy={false} onOpen={onOpen} onDiscard={onDiscard} onDiscardOther={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"Notes/a.md 열어서 확인"}));
  fireEvent.click(screen.getByRole("button",{name:"Notes/a.md 복구본 버리기"}));
  expect(onOpen).toHaveBeenCalledWith(entry); expect(onDiscard).toHaveBeenCalledWith(entry);
  await assertNoA11yViolations(container);
});
it("shows other-vault count even without current entries", async()=>{
  const onDiscardOther=vi.fn();
  const {container}=render(<RecoveryBanner entries={[]} otherVaultCount={2} busy={false} onOpen={vi.fn()} onDiscard={vi.fn()} onDiscardOther={onDiscardOther}/>);
  expect(screen.getByText(/다른 노트 폴더에서 저장하지 않은 복구본 2개/)).toBeTruthy();
  expect(screen.queryByRole("button",{name:/열어서 확인/})).toBeNull();
  fireEvent.click(screen.getByRole("button",{name:"다른 폴더 복구본 버리기"}));
  expect(onDiscardOther).toHaveBeenCalledOnce(); await assertNoA11yViolations(container);
});
it("renders nothing when every vault is empty",()=>{
  const {container}=render(<RecoveryBanner entries={[]} otherVaultCount={0} busy={false} onOpen={vi.fn()} onDiscard={vi.fn()} onDiscardOther={vi.fn()}/>);
  expect(container.firstChild).toBeNull();
});
