// Only the pinned published v0.8.1 fixture predates the delivery command split.
export function installedFixtureCommand(command, source) {
  return command === "plugin:control-center|delivery" && source === "1c97b41ee10ca0df7c062338bfe85659af025a89"
    ? "plugin:control-center|execute"
    : command;
}
