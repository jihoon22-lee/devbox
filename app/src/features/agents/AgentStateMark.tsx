import {
  CircleCheck,
  CircleX,
  LoaderCircle,
  type LucideIcon,
  MessageCircleQuestion,
  PauseCircle,
  PlayCircle,
  XCircle,
} from "lucide-react";
import s from "./Agents.module.css";
import { type AgentTask, STATE_LABEL, STATE_TONE } from "./model";

const ICON: Record<AgentTask["state"], LucideIcon> = {
  preparing: LoaderCircle,
  running: PlayCircle,
  waiting: MessageCircleQuestion,
  idle: PauseCircle,
  failed: CircleX,
  review: CircleCheck,
  closed: XCircle,
};
/**
 * The one state mark of the list row, the detail header and the grid: a coloured icon and the state name,
 * always shown. The name is the text a screen reader reads; the icon is decoration.
 */
export function AgentStateMark({ state }: { state: AgentTask["state"] }) {
  const Icon = ICON[state];
  return (
    <span className={s.state} data-state={state}>
      <Icon size={14} aria-hidden="true" style={{ color: STATE_TONE[state] }} />
      <span className={s.stateName}>{STATE_LABEL[state]}</span>
    </span>
  );
}
