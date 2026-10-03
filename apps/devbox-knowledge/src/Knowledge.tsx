import { recoveryLazy } from "./recoveryLazy";
import { NoteSessionProvider } from "@devbox/knowledge-features/notes-lifecycle";
import RecoveryBoundary from "./RecoveryBoundary";
import { Suspense } from "react";
import { ProductShell } from "@devbox/product-shell";
import { productDataAvailable } from "@devbox/product-shell/api";
import QuitGuard from "./QuitGuard";
import { Startup } from "./Startup";
const KnowledgeContent = recoveryLazy(() => import("./KnowledgeContent"));
export default function Knowledge() {
  return (
    <NoteSessionProvider>
      <QuitGuard />
      <RecoveryBoundary name="Knowledge" recoverNote>
        <ProductShell
          product="knowledge"
          renderContent={(props) => {
            const available = productDataAvailable(props.description);
            const pending = <p role="status">저장소 준비를 마친 뒤 Control Center에서 Suite 활성화를 완료해 주세요.</p>;
            if (!available && props.description.deliveryState !== "import") return pending;
            return (
              <Startup>
                {available ? (
                  <Suspense fallback={<p role="status">Knowledge 기능을 불러오고 있습니다…</p>}>
                    <KnowledgeContent {...props} />
                  </Suspense>
                ) : (
                  pending
                )}
              </Startup>
            );
          }}
        />
      </RecoveryBoundary>
    </NoteSessionProvider>
  );
}
