/**
 * 文字收斂（見 `.scratch/assignment-flow/spec.md` §決策 7）：
 * `trim` 後為空字串回傳 `null`，供可空表單欄位使用；原先 5 檔逐字重複同一段。
 */
export function textOrNull(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}
