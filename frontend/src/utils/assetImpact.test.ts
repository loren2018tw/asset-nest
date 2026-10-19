import { describe, expect, it } from "vitest";

import { summarizeAssetImpact } from "@/utils/assetImpact";

describe("summarizeAssetImpact", () => {
  it("計數介面、指派與其中保留", () => {
    const impact = summarizeAssetImpact({
      interfaces: [{ mac: "aa:bb:cc:dd:ee:ff" }, { mac: null }],
      assignments: [
        { purpose: "reservation" },
        { purpose: "static" },
        { purpose: "reservation" }
      ],
      lastSeenAt: null
    });

    expect(impact.interfaceCount).toBe(2);
    expect(impact.assignmentCount).toBe(3);
    expect(impact.reservationCount).toBe(2);
  });

  it("無介面與指派時計數為 0", () => {
    const impact = summarizeAssetImpact({
      interfaces: [],
      assignments: [],
      lastSeenAt: null
    });

    expect(impact.interfaceCount).toBe(0);
    expect(impact.assignmentCount).toBe(0);
    expect(impact.reservationCount).toBe(0);
  });

  it("lastSeenAt 為 null 顯示「—」", () => {
    const impact = summarizeAssetImpact({
      interfaces: [],
      assignments: [],
      lastSeenAt: null
    });

    expect(impact.lastSeen).toBe("—");
  });

  it("lastSeenAt 有值顯示相對時間", () => {
    const twoMinutesAgo = new Date(Date.now() - 2 * 60 * 1000).toISOString();
    const impact = summarizeAssetImpact({
      interfaces: [],
      assignments: [],
      lastSeenAt: twoMinutesAgo
    });

    expect(impact.lastSeen).toBe("2 分鐘前");
  });

  it("macLabel 以「、」連接非 null 且非空的 MAC", () => {
    const impact = summarizeAssetImpact({
      interfaces: [
        { mac: "aa:bb:cc:dd:ee:ff" },
        { mac: null },
        { mac: "" },
        { mac: "11:22:33:44:55:66" }
      ],
      assignments: [],
      lastSeenAt: null
    });

    expect(impact.macLabel).toBe("aa:bb:cc:dd:ee:ff、11:22:33:44:55:66");
  });

  it("無可用 MAC 顯示「—」", () => {
    const impact = summarizeAssetImpact({
      interfaces: [{ mac: null }, { mac: "" }],
      assignments: [],
      lastSeenAt: null
    });

    expect(impact.macLabel).toBe("—");
  });
});
