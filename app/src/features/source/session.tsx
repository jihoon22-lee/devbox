import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useRpc } from "../../rpc/RpcProvider";
import { syncDocumentTabs, useSource } from "./store";
export function SourceSession() {
  const client = useRpc();
  const query = useQueryClient();
  useEffect(() => syncDocumentTabs(), []);
  useEffect(
    () =>
      client.subscribe(
        "documents.moved",
        (_rev, moved) => {
          useSource.getState().send({ type: "rename", from: moved.from, to: moved.to });
          void query.invalidateQueries({ queryKey: ["files.tree"] });
        },
        () => {
          void query.invalidateQueries({ queryKey: ["files.tree"] });
        },
      ),
    [client, query],
  );
  return null;
}
