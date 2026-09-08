import { describe, expect, it } from "vitest";
import {
  bytesToBase64,
  clipboardImageFiles,
  droppedImageFiles,
  IMAGE_RESULT_ERROR,
  IMAGE_ERROR,
  IMAGE_TOO_LARGE_ERROR,
  imageErrorMessage,
  MAX_IMAGE_ASSET_BYTES,
  readImageBytes,
  relativeAssetDestination,
  validateImageAssetResult,
} from "./imageAssets";

const asset = "assets/" + "a".repeat(64) + ".png";

describe("image asset boundaries", () => {
  function imageTransfer(files: File[]): DataTransfer {
    return {
      files,
      items: files.map((file) => ({
        kind: "file",
        type: file.type,
        getAsFile: () => file,
      })),
    } as unknown as DataTransfer;
  }

  it("encodes bytes in bounded, deterministic base64 chunks", () => {
    expect(bytesToBase64(new Uint8Array([0, 1, 2, 253, 254, 255]))).toBe("AAEC/f7/");
  });

  it("builds a relative destination from the note directory", () => {
    expect(relativeAssetDestination("note.md", asset)).toBe(asset);
    expect(relativeAssetDestination("Notes/deep/note.md", asset)).toBe("../.." + "/" + asset);
    expect(relativeAssetDestination("../secret.md", asset)).toBeNull();
    expect(relativeAssetDestination("note.md", "../" + asset)).toBeNull();
    expect(relativeAssetDestination("note.md", "assets/" + "b".repeat(63) + ".svg")).toBeNull();
  });

  it("accepts only the exact native-generated Markdown result", () => {
    const result = {
      relativePath: asset,
      markdown: "![image](" + asset + ")",
      reused: false,
    };
    expect(validateImageAssetResult("note.md", result)).toEqual(result);
    expect(() => validateImageAssetResult("Notes/note.md", result)).toThrow(IMAGE_RESULT_ERROR);
    expect(() => validateImageAssetResult("note.md", {
      ...result,
      markdown: "![secret](https://example.test/secret)",
    })).toThrow(IMAGE_RESULT_ERROR);
  });

  it("rejects a file before reading when its declared size exceeds the bound", async () => {
    const file = new File([new Uint8Array([1])], "screenshot.png", { type: "image/png" });
    Object.defineProperty(file, "size", { configurable: true, value: MAX_IMAGE_ASSET_BYTES + 1 });
    await expect(readImageBytes(file)).rejects.toThrow(IMAGE_TOO_LARGE_ERROR);
  });

  it("bounds base64 conversion even when the helper is called directly", () => {
    expect(() => bytesToBase64(new Uint8Array(MAX_IMAGE_ASSET_BYTES + 1))).toThrow(
      IMAGE_TOO_LARGE_ERROR,
    );
  });

  it("accepts image transfer items but never treats text items as assets", () => {
    const image = new File([new Uint8Array([1])], "ignored-name.png", { type: "image/png" });
    const text = new File(["not an image"], "note.txt", { type: "text/plain" });
    const transfer = imageTransfer([image, text]);
    expect(clipboardImageFiles(transfer)).toEqual([image]);
    expect(droppedImageFiles(transfer)).toEqual([image]);
    expect(clipboardImageFiles(null)).toEqual([]);
    expect(droppedImageFiles(null)).toEqual([]);
  });

  it("redacts unexpected import failures to the fixed image error", () => {
    expect(imageErrorMessage(new Error("secret /tmp/token.png"))).toBe(IMAGE_ERROR);
  });
});
