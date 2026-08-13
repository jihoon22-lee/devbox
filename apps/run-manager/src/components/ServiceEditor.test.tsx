import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ServiceEditor from "./ServiceEditor";

afterEach(() => cleanup());

describe("ServiceEditor", () => {
  it("saves a durable service definition without runtime controls", async () => {
    const onSave = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
    const { getByLabelText, getByRole, queryByRole } = render(
      <ServiceEditor service={null} onSave={onSave} onCancel={vi.fn()} />,
    );

    fireEvent.change(getByLabelText("서비스 이름"), { target: { value: "web" } });
    fireEvent.change(getByLabelText("서비스 실행 명령"), { target: { value: "npm start" } });
    fireEvent.click(getByRole("button", { name: "서비스 저장" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "web",
        command: "npm start",
        restartPolicy: "never",
        environment: { action: "keep" },
      }),
    );
    expect(queryByRole("button", { name: /서비스 시작|서비스 중지|서비스 재시작/ })).toBeNull();
  });

  it("rejects a non-local TCP address when the optional probe is enabled", async () => {
    const onSave = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
    const { getByLabelText, getByRole } = render(
      <ServiceEditor service={null} onSave={onSave} onCancel={vi.fn()} />,
    );

    fireEvent.change(getByLabelText("서비스 이름"), { target: { value: "web" } });
    fireEvent.change(getByLabelText("서비스 실행 명령"), { target: { value: "npm start" } });
    fireEvent.click(getByRole("checkbox", { name: "TCP 헬스체크 사용" }));
    fireEvent.change(getByLabelText("TCP 헬스체크 주소"), { target: { value: "8.8.8.8" } });
    fireEvent.change(getByLabelText("TCP 헬스체크 포트"), { target: { value: "3000" } });
    fireEvent.click(getByRole("button", { name: "서비스 저장" }));

    expect(await getByRole("alert")).toBeTruthy();
    expect(onSave).not.toHaveBeenCalled();
  });
});
