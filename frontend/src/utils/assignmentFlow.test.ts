import { describe, expect, it } from "vitest";

import type { Asset, AssetInput } from "@/api/assets";
import type { AssetAssignmentSaved } from "@/api/assignments";
import { ApiError } from "@/api/client";
import type { Interface, InterfaceSaved } from "@/api/interfaces";
import type { IpQueryStatus } from "@/api/ipCandidates";
import type { KeaSyncState } from "@/api/kea";
import type { SubnetSummary } from "@/api/subnets";
import {
  assignFromAssetFlow,
  assignFromSubnetFlow,
  assignmentPurposeLabel,
  cancelAssignmentConfirmMessage,
  cancelAssignmentFlow,
  createInterfaceFlow,
  draftAssignmentFlow,
  interfaceLabel,
  interfaceNameRule,
  landAssignmentDrafts,
  parseAssignedElsewhere,
  preflightAssignmentDrafts,
  purposeOptionsFor,
  registerIpv6Flow,
  saveAssetBody,
  statusBlockMessage,
  syncInterfaces,
  type AssignedElsewhere,
  type AssignmentDraftRow,
  type FlowApi,
  type FlowDeps,
  type InterfaceRow
} from "@/utils/assignmentFlow";

// ---------- fake deps 與資料 fixtures ----------

/** fake 帳本一筆：API、confirm 與通知共用同一條呼叫序。 */
interface FakeCall {
  name: string;
  args: unknown[];
}

function callNames(calls: FakeCall[]): string[] {
  return calls.map(call => call.name);
}

function argsOf(calls: FakeCall[], name: string): unknown[][] {
  return calls.filter(call => call.name === name).map(call => call.args);
}

function asset(id: number): Asset {
  return {
    id,
    property_no: null,
    description: "測試資產",
    location: "機房",
    device_serial: null,
    brand: null,
    model: null,
    purchase_date: null,
    lifespan_years: null,
    note: null,
    tags: [],
    expired: false,
    created_at: "2026-10-19T00:00:00Z",
    updated_at: "2026-10-19T00:00:00Z"
  };
}

function iface(overrides: Partial<Interface> = {}): Interface {
  return {
    id: 11,
    asset_id: 8,
    name: "eth0",
    mac: "aa:bb:cc:dd:ee:ff",
    note: null,
    created_at: "2026-10-19T00:00:00Z",
    updated_at: "2026-10-19T00:00:00Z",
    ...overrides
  };
}

function interfaceSaved(
  overrides: Partial<InterfaceSaved> = {}
): InterfaceSaved {
  return {
    ...iface(),
    warnings: [],
    ...overrides
  };
}

function assignmentSaved(
  overrides: Partial<AssetAssignmentSaved> = {}
): AssetAssignmentSaved {
  return {
    id: 1,
    subnet_id: 3,
    address: "10.0.0.5",
    interface_id: 11,
    purpose: "static",
    hostname: null,
    created_at: "2026-10-19T00:00:00Z",
    updated_at: "2026-10-19T00:00:00Z",
    warnings: [],
    transferred: false,
    ...overrides
  };
}

function interfaceRow(overrides: Partial<InterfaceRow> = {}): InterfaceRow {
  return {
    key: 1,
    id: 11,
    name: "eth0",
    mac: "aa:bb:cc:dd:ee:ff",
    note: "",
    original: { name: "eth0", mac: "aa:bb:cc:dd:ee:ff", note: "" },
    ...overrides
  };
}

function draftRow(
  overrides: Partial<AssignmentDraftRow> = {}
): AssignmentDraftRow {
  return {
    key: 1,
    rowKey: 1,
    address: "10.0.0.5",
    purpose: "static",
    hostname: null,
    subnetId: 3,
    subnetLabel: "LAN｜10.0.0.0/24",
    ...overrides
  };
}

function status(overrides: Partial<IpQueryStatus> = {}): IpQueryStatus {
  return {
    address: "10.0.0.5",
    status: "available",
    subnet_id: 3,
    subnet_cidr: "10.0.0.0/24",
    subnet_name: "LAN",
    ...overrides
  };
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

/** 後端「位址已指派給其他介面」錯誤（真 `ApiError`，見票 10、票 15）。 */
function elsewhereError(): ApiError {
  return new ApiError(409, "位址已被指派", {
    reason: "address_assigned_elsewhere",
    asset_property_no: "P-1",
    asset_description: "NAS",
    asset_location: "機房",
    interface_name: "eth9",
    mac: "aa:bb:cc:dd:ee:99"
  });
}

/** 建 fake deps：所有 API、confirm 與通知記入同一本 `calls`（依呼叫序）。 */
function fakeDeps(
  calls: FakeCall[],
  options: {
    api?: Partial<FlowApi>;
    confirm?: (
      address: string,
      target: AssignedElsewhere
    ) => boolean | Promise<boolean>;
  } = {}
): FlowDeps {
  const api: FlowApi = {
    createAsset: async input => {
      calls.push({ name: "createAsset", args: [input] });
      return asset(42);
    },
    updateAsset: async (id, input) => {
      calls.push({ name: "updateAsset", args: [id, input] });
      return asset(id);
    },
    createInterface: async (assetId, input) => {
      calls.push({ name: "createInterface", args: [assetId, input] });
      return interfaceSaved({ id: 21 });
    },
    updateInterface: async (id, input) => {
      calls.push({ name: "updateInterface", args: [id, input] });
      return interfaceSaved({ id });
    },
    deleteInterface: async id => {
      calls.push({ name: "deleteInterface", args: [id] });
      return { kea_sync: [] };
    },
    assignFromAsset: async (assetId, input) => {
      calls.push({ name: "assignFromAsset", args: [assetId, input] });
      return assignmentSaved({ address: input.address });
    },
    assignIp: async (subnetId, address, input) => {
      calls.push({ name: "assignIp", args: [subnetId, address, input] });
      return assignmentSaved();
    },
    registerIp: async (subnetId, input) => {
      calls.push({ name: "registerIp", args: [subnetId, input] });
      return assignmentSaved();
    },
    cancelAssignment: async (subnetId, address) => {
      calls.push({ name: "cancelAssignment", args: [subnetId, address] });
      return {};
    },
    ...options.api
  };

  return {
    api,
    confirm: async (address, target) => {
      calls.push({ name: "confirm", args: [address, target] });
      if (options.confirm === undefined) {
        return true;
      }
      return options.confirm(address, target);
    },
    notify: {
      warning(message) {
        calls.push({ name: "warning", args: [message] });
      },
      kea(sync) {
        calls.push({ name: "kea", args: [sync] });
      }
    }
  };
}

// ---------- 共用純規則 ----------

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

describe("parseAssignedElsewhere", () => {
  it("非 Error 與一般 Error：回 null", () => {
    expect(parseAssignedElsewhere("boom")).toBeNull();
    expect(
      parseAssignedElsewhere({
        details: { reason: "address_assigned_elsewhere" }
      })
    ).toBeNull();
    expect(parseAssignedElsewhere(new Error("boom"))).toBeNull();
  });

  it("reason 非 address_assigned_elsewhere：回 null", () => {
    expect(
      parseAssignedElsewhere(new ApiError(409, "boom", { reason: "other" }))
    ).toBeNull();
  });

  it("取出欄位（逐字）", () => {
    expect(
      parseAssignedElsewhere(
        new ApiError(409, "boom", {
          reason: "address_assigned_elsewhere",
          asset_property_no: "P-1",
          asset_description: "NAS",
          asset_location: "機房",
          interface_name: "eth0",
          mac: "aa:bb:cc:dd:ee:ff"
        })
      )
    ).toEqual({
      asset_property_no: "P-1",
      asset_description: "NAS",
      asset_location: "機房",
      interface_name: "eth0",
      mac: "aa:bb:cc:dd:ee:ff"
    });
  });

  it("缺漏或空白欄位：描述與位置用 fallback、其餘 null", () => {
    expect(
      parseAssignedElsewhere(
        new ApiError(409, "boom", {
          reason: "address_assigned_elsewhere",
          asset_description: "",
          asset_location: null,
          asset_property_no: "",
          interface_name: 42,
          mac: {}
        })
      )
    ).toEqual({
      asset_property_no: null,
      asset_description: "未知資產",
      asset_location: "未知位置",
      interface_name: null,
      mac: null
    });
  });
});

describe("preflightAssignmentDrafts", () => {
  it("保留草稿有 MAC：通過", () => {
    expect(
      preflightAssignmentDrafts(
        [draftRow({ purpose: "reservation" })],
        [interfaceRow({ key: 1, mac: "AA-BB-CC-DD-EE-FF" })]
      )
    ).toBeNull();
  });

  it("保留草稿的列不存在或無 MAC：回逐字訊息", () => {
    const message =
      "保留指派 10.0.0.5 需要介面 MAC；請補上 MAC 或改為手動設定。";
    expect(
      preflightAssignmentDrafts(
        [draftRow({ purpose: "reservation" })],
        [interfaceRow({ key: 1, mac: "" })]
      )
    ).toBe(message);
    expect(
      preflightAssignmentDrafts(
        [draftRow({ purpose: "reservation", rowKey: 99 })],
        [interfaceRow({ key: 1 })]
      )
    ).toBe(message);
  });

  it("手動設定草稿無 MAC：放行", () => {
    expect(
      preflightAssignmentDrafts(
        [draftRow({ purpose: "static" })],
        [interfaceRow({ key: 1, mac: "" })]
      )
    ).toBeNull();
  });
});

// ---------- 資產端流程 ----------

describe("createInterfaceFlow", () => {
  it("名稱與 MAC 皆空白：回訊息且不呼叫 API", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      createInterfaceFlow(deps, 8, { name: "  ", mac: " " })
    ).resolves.toEqual({ error: "MAC 空白時名稱為必填" });
    expect(calls).toEqual([]);
  });

  it("MAC 格式錯誤：回逐字訊息且不呼叫 API", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      createInterfaceFlow(deps, 8, { name: "eth1", mac: "aa:bb:cc" })
    ).resolves.toEqual({
      error: "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）"
    });
    expect(calls).toEqual([]);
  });

  it("成功：trim 後送 textOrNull、warnings 逐筆通知、回 saved", async () => {
    const calls: FakeCall[] = [];
    const saved = interfaceSaved({
      id: 21,
      name: "eth1",
      mac: "aa:bb:cc:dd:ee:ff",
      warnings: [
        { code: "duplicate_mac", message: "MAC 重複" },
        { code: "observed", message: "MAC 觀測不符" }
      ]
    });
    const deps = fakeDeps(calls, {
      api: {
        createInterface: async (assetId, input) => {
          calls.push({ name: "createInterface", args: [assetId, input] });
          return saved;
        }
      }
    });

    await expect(
      createInterfaceFlow(deps, 8, {
        name: " eth1 ",
        mac: "AA-BB-CC-DD-EE-FF"
      })
    ).resolves.toEqual({ saved });

    expect(calls).toEqual([
      {
        name: "createInterface",
        args: [8, { name: "eth1", mac: "AA-BB-CC-DD-EE-FF", note: null }]
      },
      { name: "warning", args: ["MAC 重複"] },
      { name: "warning", args: ["MAC 觀測不符"] }
    ]);
  });

  it("API 錯誤：轉為 error 訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        createInterface: async () => {
          throw new Error("介面建立失敗");
        }
      }
    });

    await expect(
      createInterfaceFlow(deps, 8, { name: "eth1", mac: "" })
    ).resolves.toEqual({ error: "介面建立失敗" });
  });
});

describe("assignFromAssetFlow", () => {
  type Params = Parameters<typeof assignFromAssetFlow>[2];
  const base: Params = {
    text: "10.0.0.5",
    interfaceId: 11,
    purpose: "static",
    hostname: "",
    mac: "aa:bb:cc:dd:ee:ff"
  };

  it("格式錯誤：回 error 且不呼叫 API", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromAssetFlow(deps, 8, { ...base, text: "10.0.0.999" })
    ).resolves.toEqual({
      status: "error",
      message: "IP 位址格式錯誤（例：10.0.0.5、fd00::5）"
    });
    expect(calls).toEqual([]);
  });

  it("未選介面：回逐字訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromAssetFlow(deps, 8, { ...base, interfaceId: null })
    ).resolves.toEqual({ status: "error", message: "請選擇介面" });
    expect(calls).toEqual([]);
  });

  it("保留但介面無 MAC：回逐字訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromAssetFlow(deps, 8, {
        ...base,
        purpose: "reservation",
        mac: null
      })
    ).resolves.toEqual({
      status: "error",
      message: "所選介面無 MAC，不可設為保留"
    });
    expect(calls).toEqual([]);
  });

  it("v6：用途強制 static、hostname 為 null", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await assignFromAssetFlow(deps, 8, {
      ...base,
      text: "fd00::5",
      purpose: "reservation",
      hostname: "nas-1"
    });

    expect(argsOf(calls, "assignFromAsset")).toEqual([
      [
        8,
        {
          address: "fd00::5",
          interface_id: 11,
          purpose: "static",
          hostname: null,
          transfer: false
        }
      ]
    ]);
  });

  it("成功：warnings 逐筆後 kea 通知、回 saved", async () => {
    const calls: FakeCall[] = [];
    const saved = assignmentSaved({
      warnings: [{ code: "semantic", message: "語意警示" }],
      kea_sync: { status: "failed", message: "Kea 失敗" }
    });
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          return saved;
        }
      }
    });

    await expect(
      assignFromAssetFlow(deps, 8, {
        ...base,
        purpose: "reservation",
        hostname: " nas-1 "
      })
    ).resolves.toEqual({ status: "saved", saved });

    expect(callNames(calls)).toEqual(["assignFromAsset", "warning", "kea"]);
    expect(argsOf(calls, "assignFromAsset")).toEqual([
      [
        8,
        {
          address: "10.0.0.5",
          interface_id: 11,
          purpose: "reservation",
          hostname: "nas-1",
          transfer: false
        }
      ]
    ]);
    expect(argsOf(calls, "kea")).toEqual([[saved.kea_sync]]);
  });

  it("已指派他處且取消移轉：回 cancelled、不重試、不通知", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          throw elsewhereError();
        }
      },
      confirm: () => false
    });

    await expect(assignFromAssetFlow(deps, 8, base)).resolves.toEqual({
      status: "cancelled"
    });

    expect(callNames(calls)).toEqual(["assignFromAsset", "confirm"]);
    expect(argsOf(calls, "confirm")).toEqual([
      [
        "10.0.0.5",
        {
          asset_property_no: "P-1",
          asset_description: "NAS",
          asset_location: "機房",
          interface_name: "eth9",
          mac: "aa:bb:cc:dd:ee:99"
        }
      ]
    ]);
  });

  it("已指派他處且確認：以 transfer:true 重試並回 saved", async () => {
    const calls: FakeCall[] = [];
    const saved = assignmentSaved({ transferred: true });
    let attempt = 0;
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          attempt += 1;
          if (attempt === 1) {
            throw elsewhereError();
          }
          return saved;
        }
      }
    });

    await expect(assignFromAssetFlow(deps, 8, base)).resolves.toEqual({
      status: "saved",
      saved
    });

    expect(callNames(calls)).toEqual([
      "assignFromAsset",
      "confirm",
      "assignFromAsset",
      "kea"
    ]);
    expect(argsOf(calls, "assignFromAsset")).toEqual([
      [
        8,
        {
          address: "10.0.0.5",
          interface_id: 11,
          purpose: "static",
          hostname: null,
          transfer: false
        }
      ],
      [
        8,
        {
          address: "10.0.0.5",
          interface_id: 11,
          purpose: "static",
          hostname: null,
          transfer: true
        }
      ]
    ]);
  });

  it("其他 API 錯誤：回 error 且不呼叫 confirm", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          throw new Error("後端拒絕");
        }
      }
    });

    await expect(assignFromAssetFlow(deps, 8, base)).resolves.toEqual({
      status: "error",
      message: "後端拒絕"
    });
    expect(callNames(calls)).toEqual(["assignFromAsset"]);
  });
});

describe("saveAssetBody", () => {
  const input: AssetInput = {
    property_no: null,
    description: "NAS",
    location: "機房",
    device_serial: null,
    brand: null,
    model: null,
    purchase_date: null,
    lifespan_years: null,
    note: null,
    tags: []
  };

  it("無已存 id：createAsset 並回報 createdId", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      saveAssetBody(deps, { input, savedAssetId: null, editingAssetId: null })
    ).resolves.toEqual({ assetId: 42, createdId: 42 });
    expect(argsOf(calls, "createAsset")).toEqual([[input]]);
    expect(argsOf(calls, "updateAsset")).toEqual([]);
  });

  it("savedAssetId 優先於 editingAssetId：更新、createdId 為 null", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      saveAssetBody(deps, { input, savedAssetId: 7, editingAssetId: 5 })
    ).resolves.toEqual({ assetId: 7, createdId: null });
    expect(argsOf(calls, "updateAsset")).toEqual([[7, input]]);
  });

  it("僅 editingAssetId：更新該資產", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      saveAssetBody(deps, { input, savedAssetId: null, editingAssetId: 5 })
    ).resolves.toEqual({ assetId: 5, createdId: null });
    expect(argsOf(calls, "updateAsset")).toEqual([[5, input]]);
  });

  it("API 錯誤向外拋", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        createAsset: async () => {
          throw new Error("建立失敗");
        }
      }
    });

    await expect(
      saveAssetBody(deps, { input, savedAssetId: null, editingAssetId: null })
    ).rejects.toThrow("建立失敗");
  });
});

describe("syncInterfaces", () => {
  it("新增→編輯→刪除：回填、original 回寫、kea 先於 warnings 通知一次", async () => {
    const calls: FakeCall[] = [];
    const created = interfaceSaved({
      id: 21,
      name: "eth1",
      mac: null,
      note: null,
      warnings: [{ code: "duplicate_mac", message: "新介面警示" }]
    });
    const updated = interfaceSaved({
      id: 11,
      name: "eth0",
      mac: "aa:bb:cc:dd:ee:99",
      note: null,
      warnings: [{ code: "duplicate_mac", message: "介面警示" }],
      kea_sync: [{ status: "failed", message: "Kea 失敗" }]
    });
    const newRow = interfaceRow({
      key: 1,
      id: null,
      name: "eth1",
      mac: "",
      note: "",
      original: null
    });
    const changedRow = interfaceRow({
      key: 2,
      id: 11,
      name: "eth0",
      mac: "AA-BB-CC-DD-EE-99",
      note: "",
      original: { name: "eth0", mac: "aa:bb:cc:dd:ee:11", note: "" }
    });
    const sameRow = interfaceRow({
      key: 3,
      id: 12,
      name: "eth2",
      mac: "aa:bb:cc:dd:ee:12",
      note: "",
      original: { name: "eth2", mac: "aa:bb:cc:dd:ee:12", note: "" }
    });
    const deps = fakeDeps(calls, {
      api: {
        createInterface: async (assetId, input) => {
          calls.push({ name: "createInterface", args: [assetId, input] });
          return created;
        },
        updateInterface: async (id, input) => {
          calls.push({ name: "updateInterface", args: [id, input] });
          return updated;
        },
        deleteInterface: async id => {
          calls.push({ name: "deleteInterface", args: [id] });
          return { kea_sync: [{ status: "ok" }] };
        }
      }
    });

    const original = [
      iface({ id: 11, name: "eth0", mac: "aa:bb:cc:dd:ee:11" }),
      iface({ id: 12, name: "eth2", mac: "aa:bb:cc:dd:ee:12" }),
      iface({ id: 13, name: "eth3", mac: null })
    ];
    const result = await syncInterfaces(deps, 8, {
      rows: [newRow, changedRow, sameRow],
      original
    });

    expect(callNames(calls)).toEqual([
      "createInterface",
      "updateInterface",
      "deleteInterface",
      "kea",
      "warning",
      "warning"
    ]);
    expect(argsOf(calls, "createInterface")).toEqual([
      [8, { name: "eth1", mac: null, note: null }]
    ]);
    expect(argsOf(calls, "updateInterface")).toEqual([
      [11, { name: "eth0", mac: "AA-BB-CC-DD-EE-99", note: null }]
    ]);
    expect(argsOf(calls, "deleteInterface")).toEqual([[13]]);
    expect(argsOf(calls, "warning")).toEqual([["新介面警示"], ["介面警示"]]);
    expect(argsOf(calls, "kea")).toEqual([
      [{ status: "failed", message: "Kea 失敗" }]
    ]);

    expect(newRow).toEqual({
      key: 1,
      id: 21,
      name: "eth1",
      mac: "",
      note: "",
      original: { name: "eth1", mac: null, note: null }
    });
    expect(changedRow.mac).toBe("aa:bb:cc:dd:ee:99");
    expect(changedRow.original).toEqual({
      name: "eth0",
      mac: "aa:bb:cc:dd:ee:99",
      note: null
    });
    expect(sameRow.original).toEqual({
      name: "eth2",
      mac: "aa:bb:cc:dd:ee:12",
      note: ""
    });
    expect(result.original.map(item => item.id)).toEqual([11, 12, 21]);
  });

  it("無變更：不建、不改、不刪；kea 提示一次（undefined）", async () => {
    const calls: FakeCall[] = [];
    const kept = iface({ id: 11, name: "eth0", mac: "aa:bb:cc:dd:ee:11" });
    const row = interfaceRow({
      key: 1,
      id: 11,
      name: "eth0",
      mac: "aa:bb:cc:dd:ee:11",
      note: "",
      original: { name: "eth0", mac: "aa:bb:cc:dd:ee:11", note: "" }
    });
    const deps = fakeDeps(calls);

    await expect(
      syncInterfaces(deps, 8, { rows: [row], original: [kept] })
    ).resolves.toEqual({ rows: [row], original: [kept] });

    expect(calls).toEqual([{ name: "kea", args: [undefined] }]);
  });

  it("多筆 kea 失敗：取第一筆 failed 提示一次", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        updateInterface: async id => {
          const message: KeaSyncState = {
            status: "failed",
            message: `失敗 ${id}`
          };
          return interfaceSaved({ id, kea_sync: [message] });
        }
      }
    });
    const rows = [
      interfaceRow({
        key: 1,
        id: 11,
        name: "a",
        mac: "aa:bb:cc:dd:ee:11",
        note: "",
        original: { name: "a", mac: "aa:bb:cc:dd:ee:01", note: "" }
      }),
      interfaceRow({
        key: 2,
        id: 12,
        name: "b",
        mac: "aa:bb:cc:dd:ee:12",
        note: "",
        original: { name: "b", mac: "aa:bb:cc:dd:ee:02", note: "" }
      })
    ];

    await syncInterfaces(deps, 8, {
      rows,
      original: [iface({ id: 11 }), iface({ id: 12 })]
    });

    expect(argsOf(calls, "kea")).toEqual([
      [{ status: "failed", message: "失敗 11" }]
    ]);
  });

  it("API 錯誤向外拋", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        createInterface: async () => {
          throw new Error("建立失敗");
        }
      }
    });

    await expect(
      syncInterfaces(deps, 8, {
        rows: [interfaceRow({ key: 1, id: null, original: null })],
        original: []
      })
    ).rejects.toThrow("建立失敗");
  });
});

// ---------- 草稿流程 ----------

describe("draftAssignmentFlow", () => {
  type Params = Parameters<typeof draftAssignmentFlow>[0];

  function draftParams(overrides: Partial<Params> = {}): Params {
    return {
      text: "10.0.0.5",
      purpose: "static",
      hostname: "",
      mac: "aa:bb:cc:dd:ee:ff",
      existing: [],
      subnets: null,
      statusLookup: async () => null,
      ...overrides
    };
  }

  it("格式錯誤：回逐字訊息且不查狀態", async () => {
    const looked: string[] = [];
    const result = await draftAssignmentFlow(
      draftParams({
        text: "10.0.0.999",
        statusLookup: async text => {
          looked.push(text);
          return status();
        }
      })
    );

    expect(result).toEqual({
      error: "IP 位址格式錯誤（例：10.0.0.5、fd00::5）"
    });
    expect(looked).toEqual([]);
  });

  it("保留但介面無 MAC：回逐字訊息", async () => {
    const result = await draftAssignmentFlow(
      draftParams({ purpose: "reservation", mac: null })
    );
    expect(result).toEqual({ error: "所選介面無 MAC，不可設為保留" });
  });

  it("v4 狀態阻擋：池內、排除、網段外訊息逐字", async () => {
    const cases = [
      ["in_pool", "此位址在 DHCP 位址池內，不可指派"],
      ["excluded", "此位址在排除範圍內，不可指派"],
      ["out_of_subnet", "此位址不在任何網段可指派的範圍內"]
    ] as const;

    for (const [statusKind, message] of cases) {
      const result = await draftAssignmentFlow(
        draftParams({
          statusLookup: async () => status({ status: statusKind })
        })
      );
      expect(result).toEqual({ error: message });
    }
  });

  it("狀態查詢失敗：回逐字查詢訊息", async () => {
    const result = await draftAssignmentFlow(
      draftParams({ statusLookup: async () => null })
    );
    expect(result).toEqual({
      error: "無法查詢位址 10.0.0.5 的狀態；請確認格式正確，或稍後再試"
    });
  });

  it("v4 成功：回草稿（用途／hostname 推導、網段標籤）", async () => {
    const looked: string[] = [];
    const result = await draftAssignmentFlow(
      draftParams({
        purpose: "reservation",
        hostname: " nas-1 ",
        statusLookup: async text => {
          looked.push(text);
          return status();
        }
      })
    );

    expect(looked).toEqual(["10.0.0.5"]);
    expect(result).toEqual({
      draft: {
        address: "10.0.0.5",
        purpose: "reservation",
        hostname: "nas-1",
        subnetId: 3,
        subnetLabel: "LAN｜10.0.0.0/24"
      }
    });
  });

  it("v4 成功：未命名網段僅 CIDR 標籤", async () => {
    const result = await draftAssignmentFlow(
      draftParams({ statusLookup: async () => status({ subnet_name: null }) })
    );
    expect(result).toEqual({
      draft: {
        address: "10.0.0.5",
        purpose: "static",
        hostname: null,
        subnetId: 3,
        subnetLabel: "10.0.0.0/24"
      }
    });
  });

  it("v6 命中網段：不查狀態、用途 static、hostname null", async () => {
    const looked: string[] = [];
    const result = await draftAssignmentFlow(
      draftParams({
        text: "fd00::10",
        purpose: "reservation",
        hostname: "nas-1",
        subnets: [
          subnet({ id: 2, name: "v6", cidr: "fd00::/64", family: "ipv6" })
        ],
        statusLookup: async text => {
          looked.push(text);
          return status();
        }
      })
    );

    expect(looked).toEqual([]);
    expect(result).toEqual({
      draft: {
        address: "fd00::10",
        purpose: "static",
        hostname: null,
        subnetId: 2,
        subnetLabel: "v6｜fd00::/64"
      }
    });
  });

  it("v6 網段清單未載入：略過預檢，網段留 null", async () => {
    const result = await draftAssignmentFlow(
      draftParams({ text: "fd00::10", subnets: null })
    );
    expect(result).toEqual({
      draft: {
        address: "fd00::10",
        purpose: "static",
        hostname: null,
        subnetId: null,
        subnetLabel: null
      }
    });
  });

  it("v6 不在任何網段：回逐字訊息", async () => {
    const result = await draftAssignmentFlow(
      draftParams({ text: "fd00::10", subnets: [] })
    );
    expect(result).toEqual({ error: "位址 fd00::10 不在任何網段內" });
  });

  it("同網段衝突：回既有位址訊息（逐字）", async () => {
    const result = await draftAssignmentFlow(
      draftParams({
        existing: [{ address: "10.0.0.9", subnetId: 3 }],
        statusLookup: async () => status()
      })
    );
    expect(result).toEqual({
      error: "此介面在此網段已有位址 10.0.0.9（同一介面同一網段至多一個位址）"
    });
  });
});

describe("landAssignmentDrafts", () => {
  it("無草稿：不動、回空", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      landAssignmentDrafts(deps, 8, { drafts: [], rows: [] })
    ).resolves.toEqual({ drafts: [], error: null });
    expect(calls).toEqual([]);
  });

  it("依輸入序逐筆指派：成功逐筆移除、warning 後 kea", async () => {
    const calls: FakeCall[] = [];
    const first = draftRow({ key: 1, rowKey: 1 });
    const second = draftRow({
      key: 2,
      rowKey: 2,
      address: "10.0.0.6",
      purpose: "reservation",
      hostname: "nas"
    });
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          return assignmentSaved({
            address: input.address,
            warnings: [{ code: "semantic", message: `警示 ${input.address}` }],
            kea_sync: { status: "ok" }
          });
        }
      }
    });

    await expect(
      landAssignmentDrafts(deps, 8, {
        drafts: [first, second],
        rows: [
          interfaceRow({ key: 1, id: 11 }),
          interfaceRow({ key: 2, id: 12 })
        ]
      })
    ).resolves.toEqual({ drafts: [], error: null });

    expect(callNames(calls)).toEqual([
      "assignFromAsset",
      "warning",
      "kea",
      "assignFromAsset",
      "warning",
      "kea"
    ]);
    expect(argsOf(calls, "assignFromAsset")).toEqual([
      [
        8,
        {
          address: "10.0.0.5",
          interface_id: 11,
          purpose: "static",
          hostname: null,
          transfer: false
        }
      ],
      [
        8,
        {
          address: "10.0.0.6",
          interface_id: 12,
          purpose: "reservation",
          hostname: "nas",
          transfer: false
        }
      ]
    ]);
  });

  it("API 錯誤：立即停止，保留失敗與其後草稿", async () => {
    const calls: FakeCall[] = [];
    const first = draftRow({ key: 1, rowKey: 1 });
    const failing = draftRow({ key: 2, rowKey: 2, address: "10.0.0.6" });
    const later = draftRow({ key: 3, rowKey: 3, address: "10.0.0.7" });
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          if (input.address === "10.0.0.6") {
            throw new Error("後端拒絕");
          }
          return assignmentSaved({ address: input.address });
        }
      }
    });

    await expect(
      landAssignmentDrafts(deps, 8, {
        drafts: [first, failing, later],
        rows: [
          interfaceRow({ key: 1, id: 11 }),
          interfaceRow({ key: 2, id: 12 }),
          interfaceRow({ key: 3, id: 13 })
        ]
      })
    ).resolves.toEqual({ drafts: [failing, later], error: "後端拒絕" });

    expect(argsOf(calls, "assignFromAsset")).toHaveLength(2);
  });

  it("移轉取消：停止、保留草稿、回「未移轉」訊息", async () => {
    const calls: FakeCall[] = [];
    const draft = draftRow({ key: 1, rowKey: 1 });
    const later = draftRow({ key: 2, rowKey: 2, address: "10.0.0.6" });
    const deps = fakeDeps(calls, {
      api: {
        assignFromAsset: async (assetId, input) => {
          calls.push({ name: "assignFromAsset", args: [assetId, input] });
          throw elsewhereError();
        }
      },
      confirm: () => false
    });

    await expect(
      landAssignmentDrafts(deps, 8, {
        drafts: [draft, later],
        rows: [
          interfaceRow({ key: 1, id: 11 }),
          interfaceRow({ key: 2, id: 12 })
        ]
      })
    ).resolves.toEqual({
      drafts: [draft, later],
      error: "位址 10.0.0.5 未移轉，指派尚未建立。"
    });

    expect(callNames(calls)).toEqual(["assignFromAsset", "confirm"]);
  });

  it("殘留草稿（列不存在或 id 為 null）靜默清理、續行", async () => {
    const calls: FakeCall[] = [];
    const ghost = draftRow({ key: 1, rowKey: 99 });
    const unlanded = draftRow({ key: 2, rowKey: 2, address: "10.0.0.6" });
    const real = draftRow({ key: 3, rowKey: 3, address: "10.0.0.7" });
    const deps = fakeDeps(calls);

    await expect(
      landAssignmentDrafts(deps, 8, {
        drafts: [ghost, unlanded, real],
        rows: [
          interfaceRow({ key: 2, id: null }),
          interfaceRow({ key: 3, id: 13 })
        ]
      })
    ).resolves.toEqual({ drafts: [], error: null });

    expect(argsOf(calls, "assignFromAsset")).toEqual([
      [
        8,
        {
          address: "10.0.0.7",
          interface_id: 13,
          purpose: "static",
          hostname: null,
          transfer: false
        }
      ]
    ]);
  });
});

// ---------- 網段端流程 ----------

describe("assignFromSubnetFlow", () => {
  type Params = Parameters<typeof assignFromSubnetFlow>[1];

  function subnetParams(overrides: Partial<Params> = {}): Params {
    return {
      subnetId: 3,
      address: "10.0.0.5",
      isEdit: false,
      existingInterfaceId: null,
      selectedInterfaceId: 11,
      mac: "aa:bb:cc:dd:ee:ff",
      purpose: "static",
      hostname: "",
      ...overrides
    };
  }

  it("保留但介面無 MAC：回逐字訊息且不呼叫 API", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromSubnetFlow(
        deps,
        subnetParams({ purpose: "reservation", mac: null })
      )
    ).resolves.toEqual({ error: "所選介面無 MAC，不可設為保留" });
    expect(calls).toEqual([]);
  });

  it("編輯模式缺少指派資料：回逐字訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromSubnetFlow(
        deps,
        subnetParams({ isEdit: true, existingInterfaceId: null })
      )
    ).resolves.toEqual({ error: "缺少指派資料" });
    expect(calls).toEqual([]);
  });

  it("非編輯且未選介面：回逐字訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      assignFromSubnetFlow(deps, subnetParams({ selectedInterfaceId: null }))
    ).resolves.toEqual({ error: "請選擇介面" });
    expect(calls).toEqual([]);
  });

  it("成功：保留用途的 hostname 去空白、warnings 逐筆後 kea", async () => {
    const calls: FakeCall[] = [];
    const saved = assignmentSaved({
      warnings: [{ code: "semantic", message: "語意警示" }],
      kea_sync: { status: "failed", message: "Kea 失敗" }
    });
    const deps = fakeDeps(calls, {
      api: {
        assignIp: async (subnetId, address, input) => {
          calls.push({ name: "assignIp", args: [subnetId, address, input] });
          return saved;
        }
      }
    });

    await expect(
      assignFromSubnetFlow(
        deps,
        subnetParams({ purpose: "reservation", hostname: " nas-1 " })
      )
    ).resolves.toEqual({ saved });

    expect(calls).toEqual([
      {
        name: "assignIp",
        args: [
          3,
          "10.0.0.5",
          { interface_id: 11, purpose: "reservation", hostname: "nas-1" }
        ]
      },
      { name: "warning", args: ["語意警示"] },
      { name: "kea", args: [saved.kea_sync] }
    ]);
  });

  it("編輯模式：以既有指派介面送出", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await assignFromSubnetFlow(
      deps,
      subnetParams({
        isEdit: true,
        existingInterfaceId: 9,
        selectedInterfaceId: 11
      })
    );

    expect(argsOf(calls, "assignIp")).toEqual([
      [3, "10.0.0.5", { interface_id: 9, purpose: "static", hostname: null }]
    ]);
  });

  it("API 錯誤：轉為 error 訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        assignIp: async () => {
          throw new Error("位址不可指派");
        }
      }
    });

    await expect(assignFromSubnetFlow(deps, subnetParams())).resolves.toEqual({
      error: "位址不可指派"
    });
  });
});

describe("registerIpv6Flow", () => {
  it("非 IPv6：回逐字格式訊息且不呼叫 API", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      registerIpv6Flow(deps, {
        subnetId: 2,
        text: "10.0.0.5",
        selectedInterfaceId: 11
      })
    ).resolves.toEqual({ error: "IPv6 位址格式錯誤（例：fd00::10）" });
    expect(calls).toEqual([]);
  });

  it("未選介面：回逐字訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls);

    await expect(
      registerIpv6Flow(deps, {
        subnetId: 2,
        text: "fd00::10",
        selectedInterfaceId: null
      })
    ).resolves.toEqual({ error: "請選擇介面" });
    expect(calls).toEqual([]);
  });

  it("成功：warnings 逐筆、不通知 Kea、回 saved", async () => {
    const calls: FakeCall[] = [];
    const saved = assignmentSaved({
      address: "fd00::10",
      warnings: [{ code: "semantic", message: "語意警示" }],
      kea_sync: { status: "failed", message: "不該提示" }
    });
    const deps = fakeDeps(calls, {
      api: {
        registerIp: async (subnetId, input) => {
          calls.push({ name: "registerIp", args: [subnetId, input] });
          return saved;
        }
      }
    });

    await expect(
      registerIpv6Flow(deps, {
        subnetId: 2,
        text: "fd00::10",
        selectedInterfaceId: 11
      })
    ).resolves.toEqual({ saved });

    expect(calls).toEqual([
      {
        name: "registerIp",
        args: [2, { address: "fd00::10", interface_id: 11 }]
      },
      { name: "warning", args: ["語意警示"] }
    ]);
  });

  it("API 錯誤：轉為 error 訊息", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        registerIp: async () => {
          throw new Error("位址已在其他網段");
        }
      }
    });

    await expect(
      registerIpv6Flow(deps, {
        subnetId: 2,
        text: "fd00::10",
        selectedInterfaceId: 11
      })
    ).resolves.toEqual({ error: "位址已在其他網段" });
  });
});

describe("cancelAssignmentFlow", () => {
  it("成功：先提示 Kea 結果再回 ok", async () => {
    const calls: FakeCall[] = [];
    const keaSync: KeaSyncState = { status: "failed", message: "Kea 失敗" };
    const deps = fakeDeps(calls, {
      api: {
        cancelAssignment: async (subnetId, address) => {
          calls.push({ name: "cancelAssignment", args: [subnetId, address] });
          return { kea_sync: keaSync };
        }
      }
    });

    await expect(cancelAssignmentFlow(deps, 3, "10.0.0.5")).resolves.toEqual({
      ok: true
    });
    expect(calls).toEqual([
      { name: "cancelAssignment", args: [3, "10.0.0.5"] },
      { name: "kea", args: [keaSync] }
    ]);
  });

  it("錯誤：回 ok:false 與訊息、不通知 Kea", async () => {
    const calls: FakeCall[] = [];
    const deps = fakeDeps(calls, {
      api: {
        cancelAssignment: async (subnetId, address) => {
          calls.push({ name: "cancelAssignment", args: [subnetId, address] });
          throw new Error("取消失敗");
        }
      }
    });

    await expect(cancelAssignmentFlow(deps, 3, "10.0.0.5")).resolves.toEqual({
      ok: false,
      message: "取消失敗"
    });
    expect(callNames(calls)).toEqual(["cancelAssignment"]);
  });
});
