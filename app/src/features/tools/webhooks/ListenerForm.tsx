import { useState } from "react";
import { Button } from "../../../ui";
export function ListenerForm({ onStart, busy }: { onStart: (port: number, lan: boolean) => void; busy: boolean }) {
  const [port, setPort] = useState("8081");
  const [lan, setLan] = useState(false);
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        onStart(Number(port), lan);
      }}
    >
      <label>
        포트{" "}
        <input
          aria-label="포트"
          type="number"
          min={0}
          max={65535}
          value={port}
          onChange={(e) => setPort(e.target.value)}
          required
        />
      </label>
      <label>
        <input type="checkbox" checked={lan} onChange={(e) => setLan(e.target.checked)} />
        LAN에서 받기
      </label>
      {lan && (
        <p role="note">
          Windows·다른 PC에서 받으려면 WSL 포트 전달이 필요합니다. 아래 명령은 안내이며 자동 실행하지 않습니다.
        </p>
      )}
      {lan && (
        <pre>{`netsh interface portproxy add v4tov4 listenport=${port} listenaddress=0.0.0.0 connectport=${port} connectaddress=<WSL-IP>`}</pre>
      )}
      <Button type="submit" disabled={busy}>
        수신 시작
      </Button>
    </form>
  );
}
