import { apiUploadMany } from "@/api/client";

/** 清冊種類（見 spec §2）：財產／物品，僅為來源標示。 */
export type InventoryAuditKind = "property" | "item";

/** 逐檔摘要（見 spec §5.2）。 */
export interface InventoryAuditFileSummary {
  filename: string;
  kind: InventoryAuditKind;
  /** 資料列數。 */
  total: number;
  /** 略過列數。 */
  skipped: number;
}

/** 整體摘要（見 spec §5.2）。 */
export interface InventoryAuditSummary {
  list_total: number;
  list_property: number;
  list_item: number;
  system_total: number;
  matched: number;
  missing_in_system: number;
  missing_in_list: number;
}

/**
 * 「清冊有、系統沒有」的列（見 spec §5.2）。
 * 字串保留 Excel 原文（含換行、前後／全形空白），缺欄或空白為 `null`。
 */
export interface InventoryAuditMissingInSystemRow {
  file: string;
  kind: InventoryAuditKind;
  /** 該工作表實際列號（1-based）。 */
  row_number: number;
  property_no: string;
  name: string | null;
  form: string | null;
  location: string | null;
  custodian: string | null;
  note: string | null;
  /** 同鍵在清冊側出現多列。 */
  duplicate: boolean;
}

/** 「系統有、清冊沒有」的資產列（見 spec §5.2）。 */
export interface InventoryAuditMissingInListRow {
  asset_id: number;
  property_no: string;
  description: string;
  location: string;
  note: string | null;
  /** 同鍵在系統側出現多列。 */
  duplicate: boolean;
}

/** 稽核回應（見 spec §5.2）。 */
export interface InventoryAuditReport {
  files: InventoryAuditFileSummary[];
  summary: InventoryAuditSummary;
  /** 已依比對鍵升冪排序；前端不再重排。 */
  missing_in_system: InventoryAuditMissingInSystemRow[];
  /** 已依比對鍵升冪排序；前端不再重排。 */
  missing_in_list: InventoryAuditMissingInListRow[];
}

/** 檔級錯誤的逐檔原因（`ApiError.details.files` 元素；見 spec §4）。 */
export interface InventoryAuditFileError {
  filename: string;
  message: string;
}

/**
 * 上傳 1～10 個財管盤點清冊（`.xls`／`.xlsx`）執行稽核；純唯讀（見 spec §5.1）。
 * 檔級錯誤為 400、整批拒絕，逐檔原因在 `ApiError.details.files`。
 */
export function auditInventory(files: File[]): Promise<InventoryAuditReport> {
  return apiUploadMany<InventoryAuditReport>("/api/v1/inventory-audit", files);
}

/** 由 `ApiError.details` 取出逐檔原因；結構不符時回空陣列。 */
export function fileErrorsOf(
  details: Record<string, unknown> | undefined
): InventoryAuditFileError[] {
  const files = details?.files;
  if (!Array.isArray(files)) {
    return [];
  }
  return files.flatMap((entry): InventoryAuditFileError[] => {
    if (typeof entry !== "object" || entry === null) {
      return [];
    }
    const { filename, message } = entry as Record<string, unknown>;
    if (typeof filename !== "string" || typeof message !== "string") {
      return [];
    }
    return [{ filename, message }];
  });
}
