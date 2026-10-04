import { confirmAction } from "@devbox/product-shell/confirm";
import type { DevSetupConfigurationReview } from "../types";

export const DEV_SETUP_CONFIGURATION_EXPIRED = "Dev Setup 적용 미리 보기가 만료되었습니다. 구성을 다시 가져오세요.";

/** Package review validity is checked on both sides of the asynchronous decision. */
export async function confirmDevSetupApply(
  review: Pick<DevSetupConfigurationReview, "expiresAtMs" | "packages" | "canApply" | "hasChanges">,
  reportIssue: (issue: string) => void,
): Promise<boolean> {
  if (Date.now() >= review.expiresAtMs) {
    reportIssue(DEV_SETUP_CONFIGURATION_EXPIRED);
    return false;
  }
  if (review.packages.some((item) => item.currentState === "unknown" || item.action === "verify")) {
    reportIssue("확인할 수 없는 패키지 상태가 있어 적용할 수 없습니다. 설치를 제안하지 않습니다.");
    return false;
  }
  if (!review.canApply || !review.hasChanges) return false;
  if (
    !(await confirmAction(
      "정규화된 package-only 구성의 패키지 변경을 적용할까요? 네트워크와 UAC/관리자 권한이 필요할 수 있으며 자동 재부팅은 실행하지 않습니다.",
    ))
  )
    return false;
  if (Date.now() >= review.expiresAtMs) {
    reportIssue(DEV_SETUP_CONFIGURATION_EXPIRED);
    return false;
  }
  return true;
}
