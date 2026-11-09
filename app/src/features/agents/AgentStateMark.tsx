import {
  Archive,
  CircleCheck,
  CircleX,
  LoaderCircle,
  type LucideIcon,
  MessageCircleQuestion,
  PauseCircle,
  PlayCircle,
} from "lucide-react";
import s from "./Agents.module.css";
import { type AgentTask, STATE_LABEL, STATE_TONE } from "./model";

/** One shape per state: forced colours take the colour away. (`XCircle` is `CircleX` under another name.) */
const ICON: Record<AgentTask["state"], LucideIcon> = {
  preparing: LoaderCircle,
  running: PlayCircle,
  waiting: MessageCircleQuestion,
  idle: PauseCircle,
  failed: CircleX,
  review: CircleCheck,
  closed: Archive,
};
/**
 * The one state mark of the list row, the detail header and the grid: a coloured icon and the state name,
 * always shown. The name is the text a screen reader reads; the icon is decoration. `id` lets a control
 * with a name of its own (a grid tile) give the mark as its description.
 */
export function AgentStateMark({ state, id }: { state: AgentTask["state"]; id?: string }) {
  const Icon = ICON[state];
  return (
    <span id={id} className={s.state} data-state={state}>
      <Icon size={14} aria-hidden="true" style={{ color: STATE_TONE[state] }} />
      <span className={s.stateName}>{STATE_LABEL[state]}</span>
    </span>
  );
}
