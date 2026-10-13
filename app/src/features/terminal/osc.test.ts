import { parseOsc7 } from "./osc";

test("OSC7 decodes absolute file paths and rejects invalid URLs/escapes", () => {
  expect(parseOsc7("file://host/home/u/my%20app")).toBe("/home/u/my app");
  expect(parseOsc7("file:///tmp/%ED%95%9C%EA%B8%80")).toBe("/tmp/한글");
  for (const value of ["http://x/y", "garbage", "file:///bad%FF", "file:///tmp/%00", "file:///tmp?x=1"])
    expect(parseOsc7(value)).toBeNull();
});
