import { useEffect, useState } from "react";

/** 창 너비가 `width` 미만인지(요청·응답을 위아래로 놓는 기준, 07b 1100px). */
export function useNarrow(width: number): boolean {
  const [narrow, setNarrow] = useState(() => window.innerWidth < width);
  useEffect(() => {
    const update = () => setNarrow(window.innerWidth < width);
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, [width]);
  return narrow;
}
