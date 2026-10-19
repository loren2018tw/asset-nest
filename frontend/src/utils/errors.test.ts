import { describe, expect, it } from "vitest";

import { messageOf } from "@/utils/errors";

describe("messageOf", () => {
  it("Error 取 message", () => {
    expect(messageOf(new Error("壞掉了"))).toBe("壞掉了");
  });

  it("Error 子類別取 message", () => {
    expect(messageOf(new RangeError("超出範圍"))).toBe("超出範圍");
  });

  it("非 Error 以 String 轉換", () => {
    expect(messageOf("字串錯誤")).toBe("字串錯誤");
    expect(messageOf(404)).toBe("404");
    expect(messageOf(null)).toBe("null");
    expect(messageOf(undefined)).toBe("undefined");
  });
});
