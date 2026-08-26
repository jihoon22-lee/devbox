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
  { id: "json-format", group: "JSON", name: "Formatter", component: () => <TransformerTool placeholder="Paste JSON..." run={jsonFormatter()} /> },
  { id: "json-minify", group: "JSON", name: "Minifier", component: () => <TransformerTool placeholder="Paste JSON..." run={jsonMinifier()} /> },
  { id: "json-yaml", group: "JSON", name: "JSON ↔ YAML", component: JsonYamlTool },
  { id: "json-typescript", group: "JSON", name: "JSON → TypeScript", component: JsonTypescriptTool },
];

const encodingTools: ToolDef[] = [
  { id: "byte-codec", group: "Encoding", name: "UTF-8 / Base64 / Hex", component: ByteCodecTool },
  { id: "radix", group: "Encoding", name: "Radix Converter", component: RadixTool },
  { id: "html-entity-encode", group: "Encoding", name: "HTML Entity Encode", component: HtmlEntityEncoder },
  { id: "html-entity-decode", group: "Encoding", name: "HTML Entity Decode", component: HtmlEntityDecoder },
  { id: "url-encode", group: "Encoding", name: "URL Component Encode", component: UrlEncoder },
  { id: "url-decode", group: "Encoding", name: "URL Component Decode", component: UrlDecoder },
];

const timeTools: ToolDef[] = [
  { id: "timestamp", group: "Time", name: "Timestamp Converter", component: TimestampConverter },
];

const textTools: ToolDef[] = [
  { id: "case", group: "Text", name: "Case Converter", component: CaseConverter },
  { id: "lorem", group: "Text", name: "Lorem Generator", component: LoremTool },
  { id: "markdown-table", group: "Text", name: "Markdown Table Formatter", component: MarkdownTableTool },
];

const securityTools: ToolDef[] = [
  { id: "hash", group: "Security", name: "Hash (MD5/SHA)", component: HashTool },
  { id: "hmac", group: "Security", name: "HMAC Generate / Verify", component: HmacTool },
  { id: "uuid", group: "Security", name: "UUID / ULID Generator", component: UuidTool },
];

const regexTools: ToolDef[] = [{ id: "regex", group: "Regex", name: "Regex Tester", component: RegexTool }];

const diffTools: ToolDef[] = [{ id: "diff", group: "Diff", name: "Text Diff", component: DiffTool }];

const authTools: ToolDef[] = [{ id: "jwt", group: "Auth", name: "JWT Decoder", component: JwtDecoder }];

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
