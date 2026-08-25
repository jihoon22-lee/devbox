import type { ComponentType } from "react";
import { ByteCodecTool } from "./ByteCodecTool";
import { TransformerTool } from "./common";
import { DiffTool } from "./diff";
import { RegexTool } from "./regex";
import { HashTool, UuidTool } from "./security";
import { JsonYamlTool } from "./JsonYamlTool";
import {
  CaseConverter,
  jsonFormatter,
  jsonMinifier,
  JwtDecoder,
  TimestampConverter,
  UrlDecoder,
  UrlEncoder,
} from "./transformers";

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
];

const encodingTools: ToolDef[] = [
  { id: "byte-codec", group: "Encoding", name: "UTF-8 / Base64 / Hex", component: ByteCodecTool },
  { id: "url-encode", group: "Encoding", name: "URL Encode", component: UrlEncoder },
  { id: "url-decode", group: "Encoding", name: "URL Decode", component: UrlDecoder },
];

const timeTools: ToolDef[] = [
  { id: "timestamp", group: "Time", name: "Timestamp Converter", component: TimestampConverter },
];

const textTools: ToolDef[] = [
  { id: "case", group: "Text", name: "Case Converter", component: CaseConverter },
];

const securityTools: ToolDef[] = [
  { id: "hash", group: "Security", name: "Hash (MD5/SHA)", component: HashTool },
  { id: "uuid", group: "Security", name: "UUID Generator", component: UuidTool },
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
