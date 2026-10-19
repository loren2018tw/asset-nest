import { describe, expect, it } from "vitest";

import type { SubnetSummary } from "@/api/subnets";
import {
  resolveSubnetByAddress,
  sameAddress,
  sameSubnetConflict
} from "@/utils/assignmentDraft";
import { parseAddress, type ParsedAddress } from "@/utils/cidr";

/** 解析測試位址；無法解析直接失敗（測試資料錯誤）。 */
function parsed(text: string): ParsedAddress {
  const address = parseAddress(text);
  if (address === null) {
    throw new Error(`測試位址無法解析：${text}`);
  }
  return address;
}

function subnet(overrides: Partial<SubnetSummary> = {}): SubnetSummary {
  return {
    id: 1,
    cidr: "10.0.0.0/24",
    name: "LAN",
    family: "ipv4",
    used: 0,
    total: 254,
    conflicts: 0,
    ...overrides
  };
}

describe("sameAddress", () => {
  it("v4：相同與不同", () => {
    expect(sameAddress("10.0.0.5", "10.0.0.5")).toBe(true);
    expect(sameAddress("10.0.0.5", "10.0.0.6")).toBe(false);
  });

  it("v6：不同寫法視為相同", () => {
    expect(sameAddress("fd00::5", "FD00:0:0:0:0:0:0:5")).toBe(true);
    expect(sameAddress("fd00::5", "fd00::6")).toBe(false);
  });

  it("不同地址族不相等", () => {
    expect(sameAddress("10.0.0.5", "fd00::5")).toBe(false);
  });

  it("無法解析時以 trim 後原文比對", () => {
    expect(sameAddress(" x ", "x")).toBe(true);
    expect(sameAddress("x", "y")).toBe(false);
  });
});

describe("resolveSubnetByAddress", () => {
  it("命中網段：名稱｜CIDR", () => {
    const subnets = [
      subnet({ id: 2, name: "v6", cidr: "fd00::/64", family: "ipv6" })
    ];
    expect(resolveSubnetByAddress(subnets, parsed("fd00::10"))).toEqual({
      subnetId: 2,
      subnetLabel: "v6｜fd00::/64"
    });
  });

  it("未命名網段僅顯示 CIDR", () => {
    const subnets = [subnet({ id: 3, name: null })];
    expect(resolveSubnetByAddress(subnets, parsed("10.0.0.5"))).toEqual({
      subnetId: 3,
      subnetLabel: "10.0.0.0/24"
    });
  });

  it("不在任何網段回 null", () => {
    const subnets = [subnet({ id: 3, cidr: "10.9.0.0/24" })];
    expect(resolveSubnetByAddress(subnets, parsed("10.0.0.5"))).toBeNull();
  });

  it("非法 CIDR 與不同地址族皆略過", () => {
    const subnets = [
      subnet({ id: 1, cidr: "not-a-cidr" }),
      subnet({ id: 2, cidr: "fd00::/64", family: "ipv6" })
    ];
    expect(resolveSubnetByAddress(subnets, parsed("10.0.0.5"))).toBeNull();
  });
});

describe("sameSubnetConflict", () => {
  const existing = [{ address: "10.0.0.9", subnetId: 3 }];

  it("同位址＝更新，不算衝突", () => {
    expect(sameSubnetConflict(existing, "10.0.0.9", 3)).toBeNull();
  });

  it("同網段不同位址＝衝突，回既有位址", () => {
    expect(sameSubnetConflict(existing, "10.0.0.10", 3)).toBe("10.0.0.9");
  });

  it("v6 同位址的不同寫法視為更新", () => {
    const v6 = [{ address: "fd00::5", subnetId: 2 }];
    expect(sameSubnetConflict(v6, "FD00:0:0:0:0:0:0:5", 2)).toBeNull();
  });

  it("不同網段無衝突", () => {
    expect(sameSubnetConflict(existing, "10.0.0.10", 4)).toBeNull();
  });

  it("subnetId 為 null（無法判定）無衝突", () => {
    expect(sameSubnetConflict(existing, "10.0.0.10", null)).toBeNull();
  });
});
