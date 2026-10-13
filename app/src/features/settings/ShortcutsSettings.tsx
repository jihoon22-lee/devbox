import { useCommands } from "../../shell/commands";
import { Kbd } from "../../ui";
export function ShortcutsSettings() {
  const commands = useCommands((state) => state.items);
  return (
    <section aria-label="앱 단축키">
      <h3>앱 단축키</h3>
      <table>
        <tbody>
          {[...commands.values()]
            .filter((command) => command.keys?.length)
            .map((command) => (
              <tr key={command.id}>
                <td>{command.label}</td>
                <td>
                  <Kbd keys={command.keys!} />
                </td>
              </tr>
            ))}
        </tbody>
      </table>
    </section>
  );
}
