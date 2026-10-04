/**
 * 資產顯示標籤：供指派相關對話框使用（見 spec §4.3、票 15）。
 *
 * 格式為「財產編號(描述)」；財產編號為空時僅顯示描述，讓同名資產可辨識。
 */
export function assetLabel(
  propertyNo: string | null | undefined,
  description: string
): string {
  const no = propertyNo?.trim() ?? "";
  return no === "" ? description : `${no}(${description})`;
}
