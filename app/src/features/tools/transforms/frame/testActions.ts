import { fireEvent, screen } from "@testing-library/react";
export function openActions(target: HTMLElement): void {
  const control = target.closest(".tool-text-control, .tool-output-control");
  const button = control?.querySelector(".tool-actions-toggle");
  if (!button) throw new Error("missing explicit action button");
  fireEvent.click(button);
}
export function action(name: string): HTMLElement {
  const target = screen.getAllByRole("button", { name }).find((button) => button.closest('[data-tool-actions="true"]'));
  if (!target) throw new Error(`missing action ${name}`);
  return target;
}
