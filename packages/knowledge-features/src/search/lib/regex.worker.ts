// Runs away from the UI thread. The owner terminates this worker at its deadline.
self.onmessage = (event: MessageEvent<{ expression: string; names: string[] }>) => {
  try {
    const expression = new RegExp(event.data.expression, "i");
    const indices = event.data.names.flatMap((name, index) => expression.test(name) ? [index] : []);
    self.postMessage({ indices });
  } catch { self.postMessage({ error: true }); }
};
