export const API_PRODUCT_KEYS = [
  "apip-collections-v2", "apip-history-v2", "apip-environments", "devbox.api-playground.grpc-history/v1",
] as const;
export const API_LEGACY_KEYS = [
  "apip-collections-v2", "apip-collections", "apip-collections-v1-migrated",
  "apip-history-v2", "apip-history", "apip-history-v1-migrated",
  "apip-environments", "devbox.api-playground.grpc-history/v1",
] as const;
export type LegacyKey = typeof API_LEGACY_KEYS[number];
export type LegacyStorage = Record<LegacyKey, string | null>;
