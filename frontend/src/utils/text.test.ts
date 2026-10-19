import { describe, expect, it } from "vitest";

import { textOrNull } from "@/utils/text";

describe("textOrNull", () => {
  it("trim 後非空回傳文字", () => {
    expect(textOrNull("  介面名  ")).toBe("介面名");
  });

  it("trim 後為空字串回傳 null", () => {
    expect(textOrNull("")).toBeNull();
    expect(textOrNull("   ")).toBeNull();
    expect(textOrNull("\t\n ")).toBeNull();
  });

  it("保留文字內部空白", () => {
    expect(textOrNull(" a b ")).toBe("a b");
  });
});
