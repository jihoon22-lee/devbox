// The owner terminates this worker on timeout or cancellation.
self.onmessage = (event: MessageEvent<{ expression: string; text: string }>) => {
  try {
    self.postMessage({ matched: new RegExp(event.data.expression, "u").test(event.data.text) });
  } catch {
    self.postMessage({ error: true });
  }
};
