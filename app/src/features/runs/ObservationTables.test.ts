import { portBrowserUrl } from "./ObservationTables";

test.each([
  ["0.0.0.0", "http://127.0.0.1:8080"],
  ["127.0.0.1", "http://127.0.0.1:8080"],
  ["::", "http://[::1]:8080"],
  ["::1", "http://[::1]:8080"],
  ["192.168.1.2", "http://192.168.1.2:8080"],
  ["2001:db8::1", "http://[2001:db8::1]:8080"],
  ["::ffff:127.0.0.1", "http://[::ffff:7f00:1]:8080"],
])("browser uses a valid address for %s", (addr, expected) => {
  expect(portBrowserUrl({ proto: "tcp", addr, port: 8080 })).toBe(expected);
});
test("browser rejects non-TCP, invalid ports and non-IP addresses", () => {
  for (const addr of ["example.com", "abc", "127.0.0.1/path", "127.0.0.1@evil", "999.1.1.1", ":::1"])
    expect(portBrowserUrl({ proto: "tcp", addr, port: 8080 })).toBeUndefined();
  for (const port of [0, -1, 65536, 1.5])
    expect(portBrowserUrl({ proto: "tcp", addr: "127.0.0.1", port })).toBeUndefined();
  expect(portBrowserUrl({ proto: "udp", addr: "127.0.0.1", port: 8080 })).toBeUndefined();
});
