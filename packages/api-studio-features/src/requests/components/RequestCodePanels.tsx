import { lazy, Suspense } from "react";
import { buildCurl, copyRevealedCurl } from "../lib/requestPresentation";
import type { EnvVariable } from "../lib/environments";
import type { RequestTemplate } from "../types";
const CodePanel = lazy(() => import("../CodePanel").then((module) => ({ default: module.CodePanel })));
export function RequestCodePanels({
  request,
  environment,
  codeEnvironment,
  showCode,
  showCurl,
  configurationError,
  onError,
}: {
  request: RequestTemplate;
  environment: EnvVariable[];
  codeEnvironment: EnvVariable[];
  showCode: boolean;
  showCurl: boolean;
  configurationError: string | null;
  onError: (error: string | null) => void;
}) {
  return (
    <>
      {showCode && (
        <Suspense fallback={<p role="status">코드 생성 준비 중…</p>}>
          <CodePanel request={request} environment={codeEnvironment} />
        </Suspense>
      )}
      {showCurl && !configurationError && (
        <div className="curl-panel">
          <div className="io-label">
            cURL
            <button className="copy-btn" onClick={() => void navigator.clipboard.writeText(buildCurl(request))}>
              마스킹 복사
            </button>
            <button className="copy-btn" onClick={() => void copyRevealedCurl(request, environment, onError)}>
              원문 1회 복사
            </button>
          </div>
          <pre className="curl-text">{buildCurl(request) || " "}</pre>
        </div>
      )}
    </>
  );
}
