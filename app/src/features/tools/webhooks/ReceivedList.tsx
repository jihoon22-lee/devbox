import type { WebhookReceived } from "../../../rpc/gen/rpc";
export function ReceivedList({ items, select }: { items: WebhookReceived[]; select: (id: string) => void }) {
  return (
    <ul aria-label="받은 요청">
      {items.map((item) => (
        <li key={item.id}>
          <button type="button" onClick={() => select(item.id)}>
            {item.method} {item.path} · {item.sizeBytes} B{item.truncated ? " · 잘림" : ""}
          </button>
        </li>
      ))}
    </ul>
  );
}
