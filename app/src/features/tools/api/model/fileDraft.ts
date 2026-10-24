/** 요청 파일 ↔ 편집 초안. 저장할 때 가린 위치를 현재 행 번호로 되돌려 파일에 남긴다. */
import { type RequestDraft } from "./draft";
import { draftMasks, maskedPaths } from "./masked";
import type { ParsedRequestFile, RequestFile } from "./requestFile";
import { fromStored, toStored } from "./stored";

export function draftFromFile(parsed: ParsedRequestFile): RequestDraft {
  const { file } = parsed;
  const draft = fromStored(file.request, { assertions: file.assertions, captures: file.captures });
  return { ...draft, masked: draftMasks(draft, file.masked ?? []) };
}

/**
 * 화면 저장용 파일. 사용자가 본 내용을 저장하므로 파일의 `requiresSecretReview` 표시는 지운다
 * (프로젝트 루트의 digest 검토는 데몬 `api.collection_review`가 따로 기록한다).
 */
export function fileFromDraft(draft: RequestDraft, name: string, _previous?: ParsedRequestFile): RequestFile {
  const masked = maskedPaths(draft);
  return {
    version: 1,
    name,
    request: toStored(draft),
    assertions: draft.assertions,
    captures: draft.captures,
    ...(masked.length ? { masked } : {}),
  };
}
