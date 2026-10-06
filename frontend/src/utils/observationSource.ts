import type { IpSeenSource } from "@/api/ips";

/**
 * 觀測來源標籤（見 GLOSSARY.md「觀測詞彙」、ADR-0017）：
 * 本地 ARP 探測「ARP」、Kea 租約「Kea 租約」、被動監聽「ARP 被動」；
 * 未知或缺少顯示「—」。IP 清單、觀測歷史對話框與網段外觀測清單共用。
 */
export function sourceLabel(source: IpSeenSource | null | undefined): string {
  switch (source) {
    case "arp":
      return "ARP";
    case "kea_lease":
      return "Kea 租約";
    case "arp_passive":
      return "ARP 被動";
    default:
      return "—";
  }
}
