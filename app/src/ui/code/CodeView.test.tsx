import { language as activeLanguage, type LanguageSupport } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterAll, afterEach, beforeAll, expect, test, vi } from "vitest";
import { CodeView } from "./CodeView";
import * as languages from "./languages";

afterEach(cleanup);

// jsdom has no layout. Supply only Range geometry; the editor and parser stay real.
const rangeGeometry = ["getClientRects", "getBoundingClientRect"] as const;
const originalGeometry = rangeGeometry.map((name) => Object.getOwnPropertyDescriptor(Range.prototype, name));
beforeAll(() => {
  Object.defineProperties(Range.prototype, {
    getClientRects: { configurable: true, value: () => [] },
    getBoundingClientRect: { configurable: true, value: () => new DOMRect() },
  });
});
afterAll(() => {
  rangeGeometry.forEach((name, index) => {
    const original = originalGeometry[index];
    if (original) Object.defineProperty(Range.prototype, name, original);
    else Reflect.deleteProperty(Range.prototype, name);
  });
});

function editor(label = "코드") {
  const content = screen.getByRole("textbox", { name: label });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error("CodeMirror editor missing");
  return { content, view };
}

function pendingLanguage() {
  let resolve!: (value: LanguageSupport | null) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<LanguageSupport | null>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

test("renders plain text in a labelled multiline CodeMirror editor", () => {
  render(<CodeView value={"hello\nworld"} ariaLabel="코드" />);
  const { content, view } = editor();
  expect(view.state.doc.toString()).toBe("hello\nworld");
  expect(content).toHaveTextContent("hello");
  expect(content).toHaveTextContent("world");
  expect(content).toHaveAttribute("aria-multiline", "true");
  expect(view.state.facet(activeLanguage)).toBeNull();
});

test("loads JSON and renders actual syntax highlight spans", async () => {
  render(<CodeView value={'{"message": "hello", "count": 2}'} language="json" ariaLabel="코드" />);
  await waitFor(() => {
    const stringSpan = [...editor().content.querySelectorAll("span[class]")].find(
      (span) => span.textContent === '"hello"',
    );
    expect(stringSpan).toBeDefined();
    expect(editor().view.state.facet(activeLanguage)).not.toBeNull();
  });
});

test("keyboard editing calls onChange with the resulting document", () => {
  const onChange = vi.fn();
  render(<CodeView value="hello" onChange={onChange} ariaLabel="코드" />);
  const { content, view } = editor();
  act(() => view.dispatch({ selection: { anchor: 5 } }));
  fireEvent.keyDown(content, { key: "Enter", code: "Enter", keyCode: 13 });
  expect(view.state.doc.toString()).toBe("hello\n");
  expect(onChange).toHaveBeenCalledExactlyOnceWith("hello\n");
});

test("readOnly disables DOM editing and keyboard changes but permits external value updates", () => {
  const onChange = vi.fn();
  const rendered = render(<CodeView value="keep" readOnly onChange={onChange} ariaLabel="코드" />);
  const { content, view } = editor();
  expect(content).toHaveAttribute("contenteditable", "false");
  expect(content).toHaveAttribute("aria-readonly", "true");
  expect(view.state.readOnly).toBe(true);
  fireEvent.keyDown(content, { key: "Enter", code: "Enter", keyCode: 13 });
  fireEvent.keyDown(content, { key: "Backspace", code: "Backspace", keyCode: 8 });
  expect(view.state.doc.toString()).toBe("keep");
  expect(onChange).not.toHaveBeenCalled();
  rendered.rerender(<CodeView value="server" readOnly onChange={onChange} ariaLabel="코드" />);
  expect(view.state.doc.toString()).toBe("server");
  expect(onChange).not.toHaveBeenCalled();
});

test("updates value, callback, label and readOnly without replacing the editor", () => {
  const first = vi.fn();
  const next = vi.fn();
  const rendered = render(<CodeView value="one" readOnly onChange={first} ariaLabel="코드" />);
  const view = editor().view;
  rendered.rerender(<CodeView value="two" onChange={next} ariaLabel="응답" />);
  const current = editor("응답");
  expect(current.view).toBe(view);
  expect(view.state.doc.toString()).toBe("two");
  expect(current.content).toHaveAttribute("contenteditable", "true");
  expect(current.content).toHaveAttribute("aria-readonly", "false");
  expect(first).not.toHaveBeenCalled();
  expect(next).not.toHaveBeenCalled();
  act(() => view.dispatch({ selection: { anchor: 3 } }));
  fireEvent.keyDown(current.content, { key: "Enter", code: "Enter", keyCode: 13 });
  expect(next).toHaveBeenCalledExactlyOnceWith("two\n");
  expect(first).not.toHaveBeenCalled();
});

test("search controls use Korean labels and readOnly hides replacement", () => {
  render(<CodeView value="hello" readOnly ariaLabel="코드" />);
  fireEvent.keyDown(editor().content, { key: "f", code: "KeyF", ctrlKey: true });
  expect(screen.getByRole("textbox", { name: "찾기" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "다음" })).toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "바꾸기" })).toBeNull();
});

test.each(["json", "JSON", "config.json", "/project/config.json"])(
  "language loader resolves %s through language-data",
  async (name) => {
    const support = await languages.loadLanguage(name);
    expect(support?.language.name).toBe("json");
  },
);

test.each(["", "plain text", "unsupported.devboxunknown"])("language loader leaves %s as plain text", async (name) => {
  expect(await languages.loadLanguage(name)).toBeNull();
});

test("an older language request cannot replace the latest language", async () => {
  const json = await languages.loadLanguage("json");
  const javascript = await languages.loadLanguage("javascript");
  const old = pendingLanguage();
  const latest = pendingLanguage();
  vi.spyOn(languages, "loadLanguage").mockImplementation((name) => (name === "json" ? old.promise : latest.promise));
  const rendered = render(<CodeView value="{}" language="json" ariaLabel="코드" />);
  rendered.rerender(<CodeView value="{}" language="javascript" ariaLabel="코드" />);
  await act(async () => latest.resolve(javascript));
  expect(editor().view.state.facet(activeLanguage)).toBe(javascript?.language);
  await act(async () => old.resolve(json));
  expect(editor().view.state.facet(activeLanguage)).toBe(javascript?.language);
});

test("switching to plain text clears syntax and ignores a pending language", async () => {
  const json = await languages.loadLanguage("json");
  const deferred = pendingLanguage();
  const load = vi.spyOn(languages, "loadLanguage").mockResolvedValueOnce(json).mockReturnValue(deferred.promise);
  const rendered = render(<CodeView value="{}" language="json" ariaLabel="코드" />);
  await waitFor(() => expect(editor().view.state.facet(activeLanguage)).toBe(json?.language));
  rendered.rerender(<CodeView value="{}" language="javascript" ariaLabel="코드" />);
  expect(editor().view.state.facet(activeLanguage)).toBeNull();
  rendered.rerender(<CodeView value="{}" ariaLabel="코드" />);
  await act(async () => deferred.resolve(json));
  expect(editor().view.state.facet(activeLanguage)).toBeNull();
  expect(load).toHaveBeenCalledTimes(2);
});

test("failed language loading retains a usable plain text editor", async () => {
  vi.spyOn(languages, "loadLanguage").mockRejectedValue(new Error("chunk unavailable"));
  render(<CodeView value="keep" language="json" ariaLabel="코드" />);
  await act(async () => {});
  expect(editor().view.state.doc.toString()).toBe("keep");
  expect(editor().view.state.facet(activeLanguage)).toBeNull();
});

test("unmount destroys the editor and ignores pending language resolution", async () => {
  const json = await languages.loadLanguage("json");
  const deferred = pendingLanguage();
  vi.spyOn(languages, "loadLanguage").mockReturnValue(deferred.promise);
  const rendered = render(<CodeView value="{}" language="json" ariaLabel="코드" />);
  const { view } = editor();
  const destroy = vi.spyOn(view, "destroy");
  const dispatch = vi.spyOn(view, "dispatch");
  rendered.unmount();
  expect(destroy).toHaveBeenCalledOnce();
  dispatch.mockClear();
  await act(async () => deferred.resolve(json));
  expect(dispatch).not.toHaveBeenCalled();
});

test("StrictMode cleanup leaves one usable editor", () => {
  const rendered = render(
    <StrictMode>
      <CodeView value="hello" ariaLabel="코드" />
    </StrictMode>,
  );
  expect(rendered.container.querySelectorAll(".cm-editor")).toHaveLength(1);
  expect(editor().view.state.doc.toString()).toBe("hello");
});
