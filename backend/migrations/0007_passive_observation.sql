-- 票 01：網段外觀測（探索時被動 ARP 監聽；見 spec §資料庫（migration 0007）、ADR-0017）。
-- 只有被動監聽路徑寫 1；其他來源不觸碰此欄，避免同一列在來源間翻轉。
ALTER TABLE ip_presence ADD COLUMN out_of_subnet INTEGER NOT NULL DEFAULT 0;

-- 網段外觀測清單查詢用（只列 out_of_subnet=1）。
CREATE INDEX IF NOT EXISTS idx_ip_presence_out_of_subnet
    ON ip_presence(subnet_id) WHERE out_of_subnet = 1;
