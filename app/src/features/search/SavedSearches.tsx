import { useState } from "react";
import type { SearchQuery } from "../../rpc/gen/rpc";
import { messageFor } from "../../rpc/messages";
import { useRpcMutation, useRpcQuery } from "../../rpc/query";
import { Button } from "../../ui";
export function SavedSearches({ query, onSelect }: { query?: SearchQuery; onSelect(query: SearchQuery): void }) {
  const saved = useRpcQuery("search.saved", {});
  const save = useRpcMutation("search.save");
  const remove = useRpcMutation("search.unsave");
  const [name, setName] = useState("");
  const error = save.error ?? remove.error ?? saved.error;
  const busy = save.isPending || remove.isPending;
  return (
    <section aria-label="저장한 검색">
      <h3>저장한 검색</h3>
      <label>
        검색 이름 <input value={name} maxLength={100} onChange={(event) => setName(event.target.value)} />
      </label>
      <Button
        disabled={!query || !name.trim() || busy}
        onClick={() => {
          if (query)
            save.mutate(
              { name: name.trim(), query },
              {
                onSuccess: () => {
                  setName("");
                  void saved.refetch();
                },
              },
            );
        }}
      >
        검색 저장
      </Button>
      <ul>
        {saved.data?.items.map((item) => (
          <li key={item.id}>
            <Button onClick={() => onSelect(item.query)}>{item.name}</Button>{" "}
            <Button
              disabled={busy}
              onClick={() => remove.mutate({ id: item.id }, { onSuccess: () => void saved.refetch() })}
            >
              저장 취소: {item.name}
            </Button>
          </li>
        ))}
      </ul>
      {error && <p role="alert">{messageFor(error.code, error.detail).title}</p>}
    </section>
  );
}
