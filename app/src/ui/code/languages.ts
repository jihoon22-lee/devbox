import { LanguageDescription, type LanguageSupport } from "@codemirror/language";
import { languages } from "@codemirror/language-data";

/** Match an explicit name/alias first, then a file name; load only that parser. */
export async function loadLanguage(nameOrFileName: string): Promise<LanguageSupport | null> {
  const name = nameOrFileName.trim();
  if (!name) return null;
  const description =
    LanguageDescription.matchLanguageName(languages, name, false) ?? LanguageDescription.matchFilename(languages, name);
  return description ? description.load() : null;
}
