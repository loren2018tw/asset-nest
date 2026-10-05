import { relativeTime } from "./relativeTime";

/** 回收防呆提示所需欄位（`IpEntry` 的觀測子集；見票 08）。 */
export interface LastSeenHintSource {
  last_seen_at: string | null;
  last_seen_mac: string | null;
}

/**
 * 取消指派確認框的觀測提示（見票 08、ADR-0014）：
 * 顯示該位址「最後可見」與「最後 MAC」；僅提醒，不阻擋操作。
 * 回傳 HTML 片段（對話框搭配 `html: true` 呈現；內容無使用者輸入）。
 */
export function cancelAssignmentHint(source: LastSeenHintSource): string {
  const lastSeen =
    source.last_seen_at === null ? "—" : relativeTime(source.last_seen_at);
  const mac = source.last_seen_mac ?? "—";
  return `最後可見：${lastSeen}；最後 MAC：${mac}。（僅提醒，不阻擋取消指派）`;
}
