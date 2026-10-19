import { describe, expect, it } from "vitest";

import { macRule, normalizeMac } from "@/utils/mac";

describe("normalizeMac", () => {
  it("冒號格式小寫原樣回傳", () => {
    expect(normalizeMac("aa:bb:cc:dd:ee:ff")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("大寫轉小寫", () => {
    expect(normalizeMac("AA:BB:CC:DD:EE:FF")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("連字號格式", () => {
    expect(normalizeMac("aa-bb-cc-dd-ee-ff")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("點分隔格式", () => {
    expect(normalizeMac("aabb.ccdd.eeff")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("無分隔格式", () => {
    expect(normalizeMac("aabbccddeeff")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("混合分隔與大小寫", () => {
    expect(normalizeMac("AA-BB.CC:dd-ee.ff")).toBe("aa:bb:cc:dd:ee:ff");
  });

  it("長度錯誤回 null", () => {
    expect(normalizeMac("aa:bb:cc:dd:ee")).toBeNull();
    expect(normalizeMac("aa:bb:cc:dd:ee:ff:00")).toBeNull();
    expect(normalizeMac("")).toBeNull();
  });

  it("非十六進位字元回 null", () => {
    expect(normalizeMac("gg:bb:cc:dd:ee:ff")).toBeNull();
  });
});

describe("macRule", () => {
  it("空值放行", () => {
    expect(macRule(null)).toBe(true);
    expect(macRule("")).toBe(true);
    expect(macRule("   ")).toBe(true);
  });

  it("合法 MAC 放行", () => {
    expect(macRule("aa:bb:cc:dd:ee:ff")).toBe(true);
    expect(macRule("AABBCCDDEEFF")).toBe(true);
  });

  it("格式錯誤回傳逐字訊息", () => {
    expect(macRule("zz")).toBe(
      "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）"
    );
  });
});
