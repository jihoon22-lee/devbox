import { Bot, FolderGit2, NotebookPen, Play, SquareTerminal, Wrench } from "lucide-react";
import type { ReactNode } from "react";

export interface Section {
  path: string;
  label: string;
  icon: ReactNode;
  digit: number;
}

export const SECTIONS: Section[] = [
  { path: "/agents", label: "에이전트", icon: <Bot size={20} />, digit: 1 },
  { path: "/terminal", label: "터미널", icon: <SquareTerminal size={20} />, digit: 2 },
  { path: "/runs", label: "실행", icon: <Play size={20} />, digit: 3 },
  { path: "/source", label: "소스", icon: <FolderGit2 size={20} />, digit: 4 },
  { path: "/notes", label: "노트", icon: <NotebookPen size={20} />, digit: 5 },
  { path: "/tools", label: "도구", icon: <Wrench size={20} />, digit: 6 },
];
