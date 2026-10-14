import { openExternal } from "../../../shell/external";
import { useToasts } from "../../../ui";
export function openAgentPr(url: string) {
  void openExternal(url).catch(() =>
    useToasts.getState().push({ tone: "danger", title: "PR을 열지 못했습니다", body: url }),
  );
}
