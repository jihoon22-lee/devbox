import type { ComponentType } from "react";
import { ByteCodecTool } from "./ByteCodecTool";
import { TransformerTool } from "./common";
import { DiffTool } from "./diff";
import { RegexTool } from "./regex";
import { HashTool, UuidTool } from "./security";
import { HmacTool } from "./HmacTool";
import { JsonYamlTool } from "./JsonYamlTool";
import { JsonTypescriptTool } from "./JsonTypescriptTool";
import { LoremTool } from "./LoremTool";
import { MarkdownTableTool } from "./MarkdownTableTool";
import { RadixTool } from "./RadixTool";
import { QrTool } from "./QrTool";
import {
  CaseConverter,
  HtmlEntityDecoder,
  HtmlEntityEncoder,
  jsonFormatter,
  jsonMinifier,
  TimestampConverter,
  UrlDecoder,
  UrlEncoder,
} from "./transformers";
import { JwtDecoder } from "./JwtTool";

export interface ToolDef {
  id: string;
  group: string;
  name: string;
  component: ComponentType;
}

const jsonTools: ToolDef[] = [
  { id: "json-format", group: "JSON", name: "포매터", component: () => <TransformerTool placeholder="JSON을 붙여넣으세요..." run={jsonFormatter()} /> },
  { id: "json-minify", group: "JSON", name: "최소화", component: () => <TransformerTool placeholder="JSON을 붙여넣으세요..." run={jsonMinifier()} /> },
  { id: "json-yaml", group: "JSON", name: "JSON ↔ YAML", component: JsonYamlTool },
  { id: "json-typescript", group: "JSON", name: "JSON → TypeScript", component: JsonTypescriptTool },
];

const encodingTools: ToolDef[] = [
  { id: "byte-codec", group: "인코딩", name: "UTF-8 / Base64 / Hex", component: ByteCodecTool },
  { id: "radix", group: "인코딩", name: "진법 변환기", component: RadixTool },
  { id: "html-entity-encode", group: "인코딩", name: "HTML 엔터티 인코딩", component: HtmlEntityEncoder },
  { id: "html-entity-decode", group: "인코딩", name: "HTML 엔터티 디코딩", component: HtmlEntityDecoder },
  { id: "url-encode", group: "인코딩", name: "URL 컴포넌트 인코딩", component: UrlEncoder },
  { id: "url-decode", group: "인코딩", name: "URL 컴포넌트 디코딩", component: UrlDecoder },
  { id: "qr", group: "인코딩", name: "QR 생성기", component: QrTool },
];

const timeTools: ToolDef[] = [
  { id: "timestamp", group: "시간", name: "타임스탬프 변환기", component: TimestampConverter },
];

const textTools: ToolDef[] = [
  { id: "case", group: "텍스트", name: "대소문자 변환기", component: CaseConverter },
  { id: "lorem", group: "텍스트", name: "Lorem 생성기", component: LoremTool },
  { id: "markdown-table", group: "텍스트", name: "Markdown 표 포매터", component: MarkdownTableTool },
];

const securityTools: ToolDef[] = [
  { id: "hash", group: "보안", name: "해시 (MD5/SHA)", component: HashTool },
  { id: "hmac", group: "보안", name: "HMAC 생성 / 검증", component: HmacTool },
  { id: "uuid", group: "보안", name: "UUID / ULID 생성기", component: UuidTool },
];

const regexTools: ToolDef[] = [{ id: "regex", group: "정규식", name: "정규식 테스터", component: RegexTool }];

const diffTools: ToolDef[] = [{ id: "diff", group: "차이", name: "텍스트 차이", component: DiffTool }];

const authTools: ToolDef[] = [{ id: "jwt", group: "인증", name: "JWT 디코더", component: JwtDecoder }];

export const TOOLS: ToolDef[] = [
  ...jsonTools,
  ...encodingTools,
  ...timeTools,
  ...textTools,
  ...securityTools,
  ...regexTools,
  ...diffTools,
  ...authTools,
];

export const GROUPS = [...new Set(TOOLS.map((t) => t.group))];
