import { useRpcQuery } from "../../rpc/query";
import { QueryResult } from "./QueryResult";
import { Update } from "./Update";
export function AboutSettings() {
  const info = useRpcQuery("system.info", {});
  return (
    <>
      <Update />
      <section aria-label="데몬 정보">
        <h3>데몬 정보</h3>
        <QueryResult pending={info.isPending} error={info.error} retry={() => void info.refetch()}>
          {info.data && (
            <p>
              데몬 {info.data.version} · 인스턴스 {info.data.instance} · 데이터 {info.data.dataDir}
            </p>
          )}
        </QueryResult>
      </section>
      <p>devbox · 개인 개발 허브</p>
    </>
  );
}
