/**
 * MAC 位址收斂（見 `.scratch/assignment-flow/spec.md` §決策 7／§介面
 * 「共用純規則」）：三個對話框逐字重複的輸入正規化與驗證規則。
 */

/** 將常見 MAC 輸入格式正規化為小寫冒號格式；無法解析回傳 `null`。 */
export function normalizeMac(text: string): string | null {
  const hex = text.replace(/[:.-]/g, "").toLowerCase();
  if (!/^[0-9a-f]{12}$/.test(hex)) {
    return null;
  }
  return hex.match(/.{2}/g)?.join(":") ?? null;
}

/** MAC 欄位驗證規則：空值放行；格式錯誤回傳訊息（逐字沿用）。 */
export function macRule(value: string | null): true | string {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }
  return (
    normalizeMac(text) !== null ||
    "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）"
  );
}
