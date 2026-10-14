import { parseOsc7 } from "./osc";

test("OSC7 decodes absolute file paths and rejects invalid URLs/escapes", () => {
  expect(parseOsc7("file://host/home/u/my%20app", "host")).toBe("/home/u/my app");
  expect(parseOsc7("file:///tmp/%ED%95%9C%EA%B8%80")).toBe("/tmp/한글");
  for (const value of ["http://x/y", "garbage", "file:///bad%FF", "file:///tmp/%00", "file:///tmp?x=1"])
    expect(parseOsc7(value)).toBeNull();
});

test("OSC7 only accepts hostless, localhost or the confirmed daemon hostname", () => {
  expect(parseOsc7("file://remote-host/home/u/my%20app", "local-wsl")).toBeNull();
  expect(parseOsc7("file://local-wsl/home/u")).toBeNull();
  expect(parseOsc7("file://localhost/tmp")).toBe("/tmp");
  expect(parseOsc7("file://LOCALHOST./tmp")).toBe("/tmp");
  expect(parseOsc7("file://LOCAL-WSL./home/u", "local-wsl")).toBe("/home/u");
  expect(parseOsc7("file://local-wsl/home/u", "LOCAL-WSL.")).toBe("/home/u");
  for (const host of ["local-wsl.evil", "local-wsl-alias", "127.0.0.1", "[::1]"])
    expect(parseOsc7(`file://${host}/home/u`, "local-wsl")).toBeNull();
});
