import { describe, expect, it } from "vitest";
import { nextTabTitle } from "./tabTitle";

describe("nextTabTitle", () => {
  it("같은 이름의 탭이 없으면 distro 이름을 그대로 쓴다", () => {
    expect(nextTabTitle([], "Ubuntu")).toBe("Ubuntu");
    expect(nextTabTitle(["Debian"], "Ubuntu")).toBe("Ubuntu");
  });

  it("이미 하나 있으면 '이름 2'가 된다", () => {
    expect(nextTabTitle(["Ubuntu"], "Ubuntu")).toBe("Ubuntu 2");
  });

  it("'이름 2'까지 있으면 다음은 '이름 3'이다", () => {
    expect(nextTabTitle(["Ubuntu", "Ubuntu 2"], "Ubuntu")).toBe("Ubuntu 3");
  });

  it("탭이 닫혀서 번호가 비어도(빠진 번호가 있어도) 존재하는 개수만큼만 센다", () => {
    // "Ubuntu"와 "Ubuntu 3"만 남아있어도(즉 2가 없어도) count=2개이므로 다음은 "Ubuntu 3"이 아니라
    // 규칙상 "count===0?distro:count+1"이라 개수 기반으로만 계산된다.
    expect(nextTabTitle(["Ubuntu", "Ubuntu 3"], "Ubuntu")).toBe("Ubuntu 3");
  });

  it("다른 distro의 탭 이름은 세지 않는다", () => {
    expect(nextTabTitle(["Ubuntu", "Debian", "Debian 2"], "Ubuntu")).toBe("Ubuntu 2");
  });

  it("정규식 특수문자(.)가 포함된 distro 이름도 안전하게 이스케이프된다", () => {
    const distro = "node.js";
    expect(nextTabTitle([], distro)).toBe(distro);
    expect(nextTabTitle([distro], distro)).toBe(`${distro} 2`);
    // 이스케이프가 안 되면 정규식의 '.'이 임의의 한 글자에 매치되어, 점 대신 다른
    // 글자가 온 이름("nodeXjs")까지 같은 distro로 잘못 세게 된다.
    expect(nextTabTitle(["nodeXjs"], distro)).toBe(distro);
  });

  it("'이름 2'처럼 보이지만 실제로는 다른 distro인 이름은 세지 않는다", () => {
    expect(nextTabTitle(["Ubuntu 2 extra"], "Ubuntu")).toBe("Ubuntu");
  });
});
