import { describe, expect, it } from "vitest";

import {
  assignmentPurposeLabel,
  cancelAssignmentConfirmMessage,
  interfaceLabel,
  interfaceNameRule,
  purposeOptionsFor,
  statusBlockMessage
} from "@/utils/assignmentFlow";

describe("interfaceLabel", () => {
  it("有名稱與 MAC", () => {
    expect(interfaceLabel("eth0", "aa:bb:cc:dd:ee:ff")).toBe(
      "eth0 ｜ aa:bb:cc:dd:ee:ff"
    );
  });

  it("名稱缺漏以「未命名」表示", () => {
    expect(interfaceLabel(null, "aa:bb:cc:dd:ee:ff")).toBe(
      "未命名 ｜ aa:bb:cc:dd:ee:ff"
    );
  });

  it("無 MAC 時標示", () => {
    expect(interfaceLabel("eth0", null)).toBe("eth0（無 MAC）");
  });

  it("名稱與 MAC 皆缺", () => {
    expect(interfaceLabel(null, null)).toBe("未命名（無 MAC）");
  });
});

describe("assignmentPurposeLabel", () => {
  it("保留與手動設定兩種標籤", () => {
    expect(assignmentPurposeLabel("reservation")).toBe("保留");
    expect(assignmentPurposeLabel("static")).toBe("手動設定");
  });
});

describe("purposeOptionsFor", () => {
  it("有 MAC：兩個選項皆可用（文案逐字）", () => {
    expect(purposeOptionsFor("aa:bb:cc:dd:ee:ff")).toEqual([
      { label: "手動設定（static）", value: "static", disable: false },
      {
        label: "DHCPv4 保留（reservation）",
        value: "reservation",
        disable: false
      }
    ]);
  });

  it("無 MAC：保留選項停用", () => {
    expect(purposeOptionsFor(null)).toEqual([
      { label: "手動設定（static）", value: "static", disable: false },
      {
        label: "DHCPv4 保留（reservation）",
        value: "reservation",
        disable: true
      }
    ]);
  });
});

describe("interfaceNameRule", () => {
  it("有 MAC：名稱可空", () => {
    const rule = interfaceNameRule("aa:bb:cc:dd:ee:ff");
    expect(rule("")).toBe(true);
    expect(rule(null)).toBe(true);
    expect(rule("   ")).toBe(true);
    expect(rule("eth0")).toBe(true);
  });

  it("無 MAC：名稱必填（訊息逐字）", () => {
    const rule = interfaceNameRule("");
    expect(rule("eth0")).toBe(true);
    expect(rule("")).toBe("MAC 空白時名稱為必填");
    expect(rule(null)).toBe("MAC 空白時名稱為必填");
    expect(rule("   ")).toBe("MAC 空白時名稱為必填");
  });
});

describe("statusBlockMessage", () => {
  it("池內、排除、網段外各有逐字訊息", () => {
    expect(statusBlockMessage("in_pool")).toBe(
      "此位址在 DHCP 位址池內，不可指派"
    );
    expect(statusBlockMessage("excluded")).toBe("此位址在排除範圍內，不可指派");
    expect(statusBlockMessage("out_of_subnet")).toBe(
      "此位址不在任何網段可指派的範圍內"
    );
  });

  it("其餘狀態不阻擋", () => {
    expect(statusBlockMessage("available")).toBeNull();
    expect(statusBlockMessage("static")).toBeNull();
    expect(statusBlockMessage("reservation")).toBeNull();
  });
});

describe("cancelAssignmentConfirmMessage", () => {
  it("v4：取消後回到可用", () => {
    expect(cancelAssignmentConfirmMessage("10.0.0.5")).toBe(
      "確定要取消 10.0.0.5 的指派？取消後該位址回到「可用」。"
    );
  });

  it("v6：取消後自登錄清單移除", () => {
    expect(cancelAssignmentConfirmMessage("fd00::5")).toBe(
      "確定要取消 fd00::5 的指派？取消後該位址將自登錄清單移除。"
    );
  });

  it("位址為傳入原文（含空白不修飾；無法解析視同 v4 文案）", () => {
    expect(cancelAssignmentConfirmMessage(" not-an-address ")).toBe(
      "確定要取消  not-an-address  的指派？取消後該位址回到「可用」。"
    );
  });
});
