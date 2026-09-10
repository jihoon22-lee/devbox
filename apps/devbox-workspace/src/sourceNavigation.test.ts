import {expect,it} from "vitest";
import {sourceFilePath} from "./sourceNavigation";
it("rejects Git absolute/escape/device/stream paths before forwarding a Files request",()=>{
  for(const path of ["/outside","../outside","src/../../outside","C:/outside","//server/share","src\\outside","src/file:stream","src/./file","src//file","src/line\nfile"])
    expect(sourceFilePath("C:/project",path)).toBeNull();
  expect(sourceFilePath("C:/project/","한글 폴더/파일.ts")).toBe("C:/project/한글 폴더/파일.ts");
});
