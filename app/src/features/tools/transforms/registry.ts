import { type ComponentType, createElement } from "react";
import { CATALOG, type ToolId } from "./catalog";
import { TransformerTool } from "./frame";
import { jsonFormatter, jsonMinifier } from "./lib/transformers";
import { ByteCodecTool } from "./tools/ByteCodecTool";
import { DiffTool } from "./tools/diff";
import { HmacTool } from "./tools/HmacTool";
import { JsonTypescriptTool } from "./tools/JsonTypescriptTool";
import { JsonYamlTool } from "./tools/JsonYamlTool";
import { JwtDecoder } from "./tools/JwtTool";
import { LoremTool } from "./tools/LoremTool";
import { MarkdownTableTool } from "./tools/MarkdownTableTool";
import { QrTool } from "./tools/QrTool";
import { RadixTool } from "./tools/RadixTool";
import { RegexTool } from "./tools/RegexTool";
import { HashTool, UuidTool } from "./tools/security";
import {
  CaseConverter,
  HtmlEntityDecoder,
  HtmlEntityEncoder,
  TimestampConverter,
  UrlDecoder,
  UrlEncoder,
} from "./tools/TextTools";

const components: Record<ToolId, ComponentType> = {
  "json-format": () => createElement(TransformerTool, { placeholder: "JSON을 붙여넣으세요...", run: jsonFormatter() }),
  "json-minify": () => createElement(TransformerTool, { placeholder: "JSON을 붙여넣으세요...", run: jsonMinifier() }),
  "json-yaml": JsonYamlTool,
  "json-typescript": JsonTypescriptTool,
  "byte-codec": ByteCodecTool,
  radix: RadixTool,
  "html-entity-encode": HtmlEntityEncoder,
  "html-entity-decode": HtmlEntityDecoder,
  "url-encode": UrlEncoder,
  "url-decode": UrlDecoder,
  qr: QrTool,
  timestamp: TimestampConverter,
  case: CaseConverter,
  lorem: LoremTool,
  "markdown-table": MarkdownTableTool,
  hash: HashTool,
  hmac: HmacTool,
  uuid: UuidTool,
  regex: RegexTool,
  diff: DiffTool,
  jwt: JwtDecoder,
};
export const TOOLS = CATALOG.map((tool) => ({ ...tool, component: components[tool.id] }));
