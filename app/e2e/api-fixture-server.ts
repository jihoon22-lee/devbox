/** API 도구 E2E용 소유 fixture 서버(127.0.0.1:0, node http). 로그인할 때마다 새 토큰을 주고 받은 값을 기록한다. */
import { randomBytes } from "node:crypto";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

export class ApiFixture {
  readonly issued: string[] = [];
  readonly received: { path: string; session: string | undefined; trace: string | undefined }[] = [];
  private server?: Server;
  url = "";

  async start(): Promise<this> {
    this.server = createServer((request, response) => {
      const path = (request.url ?? "/").split("?")[0]!;
      const session = request.headers["x-session"];
      const trace = request.headers["x-trace"];
      this.received.push({
        path,
        session: typeof session === "string" ? session : undefined,
        trace: typeof trace === "string" ? trace : undefined,
      });
      const json = (status: number, body: unknown) => {
        response.writeHead(status, { "content-type": "application/json" });
        response.end(JSON.stringify(body));
      };
      request.resume();
      request.on("end", () => {
        if (path.startsWith("/hello")) return json(200, { hello: "world", path });
        if (path === "/login" && request.method === "POST") {
          const token = `fixture-${randomBytes(8).toString("hex")}`;
          this.issued.push(token);
          return json(200, { token });
        }
        if (path === "/me") {
          const latest = this.issued.at(-1);
          return latest && session === latest ? json(200, { user: "e2e" }) : json(401, { error: "unauthorized" });
        }
        json(404, { error: "not found" });
      });
    });
    await new Promise<void>((resolve) => this.server!.listen(0, "127.0.0.1", resolve));
    this.url = `http://127.0.0.1:${(this.server!.address() as AddressInfo).port}`;
    return this;
  }

  async stop(): Promise<void> {
    const server = this.server;
    this.server = undefined;
    if (server) await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}
