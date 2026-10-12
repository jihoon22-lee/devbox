import { viteAnnounced } from "../e2e/vite-readiness";

const colored =
  "\u001b[32m  ➜\u001b[39m  \u001b[1mLocal:\u001b[22m   \u001b[36mhttp://localhost:\u001b[1m1460\u001b[22m/\u001b[39m\n";

test("Vite listener announcement survives ANSI sequences split across output chunks", () => {
  expect(viteAnnounced(colored, 1460)).toBe(true);
  for (let split = 0; split < colored.length; split++) {
    const chunks = [colored.slice(0, split), colored.slice(split)];
    expect(viteAnnounced(chunks.join(""), 1460)).toBe(true);
  }
  expect(viteAnnounced("  ➜  Local:   http://127.0.0.1:1450/\n", 1450)).toBe(true);
});

test.each([
  "  ➜  Local: http://localhost:1461/\n",
  "  ➜  Local: http://localhost:14600/\n",
  "Local:\nhttp://localhost:1460/\n",
  "Local: error\nNetwork: http://localhost:1460/\n",
  "Local: http://foreign.example:1460/\n",
  "Local: http://localhost:1460/extra\n",
  "Local: http://localhost:1460",
])("rejects a different or malformed announcement: %s", (log) => {
  expect(viteAnnounced(log, 1460)).toBe(false);
});
