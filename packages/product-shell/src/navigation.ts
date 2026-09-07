export interface Navigation { entries: string[]; cursor: number }
export const HISTORY_LIMIT = 64;
export function navigate(state: Navigation, route: string): Navigation {
  if (state.entries[state.cursor] === route) return state;
  const entries = [...state.entries.slice(0, state.cursor + 1), route].slice(-HISTORY_LIMIT);
  return { entries, cursor: entries.length - 1 };
}
export function traverse(state: Navigation, offset: number): Navigation {
  return { ...state, cursor: Math.max(0, Math.min(state.entries.length - 1, state.cursor + offset)) };
}
