import { TransformerTool } from "../frame";
import {
  convertCase,
  convertTimestamp,
  runHtmlEntityDecode,
  runHtmlEntityEncode,
  runUrlDecode,
  runUrlEncode,
} from "../lib/transformers";
export function UrlEncoder() {
  return (
    <TransformerTool
      placeholder="URL 컴포넌트로 인코딩할 텍스트를 입력하세요..."
      run={runUrlEncode}
      clearOutputOnInput
    />
  );
}

export function UrlDecoder() {
  return (
    <TransformerTool
      placeholder="URL 컴포넌트로 인코딩된 텍스트를 입력하세요..."
      run={runUrlDecode}
      clearOutputOnInput
    />
  );
}

export function HtmlEntityEncoder() {
  return (
    <TransformerTool
      placeholder="HTML 엔터티로 인코딩할 텍스트를 입력하세요..."
      run={runHtmlEntityEncode}
      clearOutputOnInput
    />
  );
}

export function HtmlEntityDecoder() {
  return (
    <TransformerTool
      placeholder="HTML 엔터티로 인코딩된 텍스트를 입력하세요..."
      run={runHtmlEntityDecode}
      clearOutputOnInput
    />
  );
}

export function TimestampConverter() {
  return <TransformerTool placeholder="Unix 타임스탬프(초) 또는 ISO 날짜를 입력하세요..." run={convertTimestamp} />;
}

export function CaseConverter() {
  return (
    <TransformerTool
      placeholder="변환할 텍스트를 입력하세요..."
      run={(i) => Promise.resolve({ output: convertCase(i) })}
    />
  );
}
