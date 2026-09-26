import type { ProblemResolution } from "./ProblemResolution";
import type { ProblemsReply } from "./ProblemsReply";

export type ProblemsResults = {
  resolve: ProblemResolution;
  snapshot: ProblemsReply;
};
