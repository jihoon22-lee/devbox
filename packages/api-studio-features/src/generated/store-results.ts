import type { Stored } from "./Stored";

export type StoreResults = {
  load: Stored | null;
  save: number;
};
