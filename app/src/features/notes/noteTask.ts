const LIMIT = 64 * 1024;
export function noteTaskPrompt(text: string, selected: string): { prompt: string; truncated: boolean } {
  const prompt = selected || text;
  const bytes = new TextEncoder().encode(prompt);
  if (bytes.length <= LIMIT) return { prompt, truncated: false };
  let end = LIMIT;
  while (end > 0 && (bytes[end]! & 0xc0) === 0x80) end -= 1;
  return { prompt: new TextDecoder().decode(bytes.subarray(0, end)), truncated: true };
}
