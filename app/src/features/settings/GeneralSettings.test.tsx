import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { GeneralSettings } from "./GeneralSettings";
import { setTerminalFont, setUiScale } from "./preferences";

test("general settings apply and persist bounded appearance preferences", () => {
  render(<GeneralSettings />);
  fireEvent.change(screen.getByLabelText("테마"), { target: { value: "light" } });
  expect(document.documentElement.dataset.theme).toBe("light");
  fireEvent.change(screen.getByLabelText("UI 배율 (%)"), { target: { value: "150" } });
  fireEvent.blur(screen.getByLabelText("UI 배율 (%)"));
  expect(document.documentElement.style.zoom).toBe("1.5");
  fireEvent.change(screen.getByLabelText("터미널 글꼴 크기"), { target: { value: "18" } });
  fireEvent.blur(screen.getByLabelText("터미널 글꼴 크기"));
  expect(localStorage.getItem("devbox.terminalFont")).toBe("18");
});

beforeEach(() => {
  setTerminalFont(13);
  setUiScale(1);
});
test("font numeric editing accepts clear then type 20 on blur", async () => {
  render(<GeneralSettings />);
  const input = screen.getByLabelText("터미널 글꼴 크기");
  await userEvent.clear(input);
  await userEvent.type(input, "20");
  await userEvent.tab();
  expect(input).toHaveValue(20);
  expect(localStorage.getItem("devbox.terminalFont")).toBe("20");
});
test("scale numeric editing accepts clear then type 125 on blur", async () => {
  render(<GeneralSettings />);
  const input = screen.getByLabelText("UI 배율 (%)");
  await userEvent.clear(input);
  await userEvent.type(input, "125");
  await userEvent.tab();
  expect(input).toHaveValue(125);
  expect(document.documentElement.style.zoom).toBe("1.25");
});

test("numeric drafts keep the current value for empty or invalid input and clamp only on commit", async () => {
  render(<GeneralSettings />);
  const font = screen.getByLabelText("터미널 글꼴 크기");
  await userEvent.clear(font);
  expect(localStorage.getItem("devbox.terminalFont")).toBe("13");
  await userEvent.tab();
  expect(font).toHaveValue(13);
  fireEvent.change(font, { target: { value: "invalid" } });
  fireEvent.blur(font);
  expect(font).toHaveValue(13);
  await userEvent.clear(font);
  await userEvent.type(font, "200");
  expect(font).toHaveValue(200);
  expect(localStorage.getItem("devbox.terminalFont")).toBe("13");
  await userEvent.keyboard("{Enter}");
  expect(font).toHaveValue(28);
  const scale = screen.getByLabelText("UI 배율 (%)");
  await userEvent.clear(scale);
  await userEvent.type(scale, "80");
  expect(document.documentElement.style.zoom).toBe("1");
  await userEvent.tab();
  expect(scale).toHaveValue(90);
  expect(document.documentElement.style.zoom).toBe("0.9");
});
test("external preferences changes replace displayed numeric drafts", async () => {
  render(<GeneralSettings />);
  const font = screen.getByLabelText("터미널 글꼴 크기");
  const scale = screen.getByLabelText("UI 배율 (%)");
  await userEvent.clear(font);
  await userEvent.type(font, "20");
  act(() => setTerminalFont(17, false));
  expect(font).toHaveValue(17);
  await userEvent.clear(scale);
  await userEvent.type(scale, "125");
  act(() => setUiScale(1.4));
  expect(scale).toHaveValue(140);
  await userEvent.tab();
  expect(document.documentElement.style.zoom).toBe("1.4");
});
