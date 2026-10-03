import { useState } from "react";
import { makeRequest, nativeMode } from "@devbox/product-shell/api";
import type { ShellContentProps } from "@devbox/product-shell";
import { deliveryCall } from "./delivery";
import Health from "./Health";
import catalog from "../../../apps/products.json";
export default function SetupHealth({
  description,
  route,
  onRecorded,
  busy,
}: Pick<ShellContentProps, "description" | "route"> & { onRecorded: () => Promise<void>; busy: boolean }) {
  const [opening, setOpening] = useState<string | null>(null),
    [error, setError] = useState("");
  const open = async (product: string) => {
    if (opening || busy || !nativeMode) return;
    setOpening(product);
    setError("");
    try {
      const header = makeRequest(description.handshake, route, Date.now(), description.context);
      const result = await deliveryCall(header, "open_setup_product", { product });
      if (!result.value.opened) throw new Error("not opened");
    } catch {
      setError("제품을 열지 못했습니다. 설치 상태를 확인한 뒤 다시 시도해 주세요.");
    } finally {
      setOpening(null);
    }
  };
  return (
    <>
      <div className="setup-products">
        {catalog.products
          .filter((p) => p.id !== "control-center")
          .map((product) => (
            <button key={product.id} disabled={!!opening || busy || !nativeMode} onClick={() => void open(product.id)}>
              {opening === product.id ? "여는 중…" : `${product.label} 열기`}
            </button>
          ))}
      </div>
      {error && <p role="alert">{error}</p>}
      <Health description={description} route={route} onRecorded={onRecorded} />
    </>
  );
}
