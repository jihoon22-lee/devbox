import { productDataAvailable, type Description } from "./api";

/** UI availability mirrors native delivery admission; it grants no authority. */
export function deriveAvailability(description: Description) {
  const data = productDataAvailable(description);
  return {
    data,
    incoming: data,
    reconnect: data && description.agent !== "unsupported",
    setupRequired: !data,
  };
}
