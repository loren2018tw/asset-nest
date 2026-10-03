-- 票 02：資產介面管理。interfaces 資料表（見 spec §2.2、§2.5）。
CREATE TABLE IF NOT EXISTS interfaces (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id   INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    name       TEXT,
    mac        TEXT,
    note       TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    -- MAC 空白的介面必須有名稱（見 spec §3.1）；mac 儲存為正規化小寫冒號格式。
    CHECK (
        (mac IS NOT NULL AND trim(mac) <> '')
        OR (name IS NOT NULL AND trim(name) <> '')
    )
);

-- 依資產取介面清單，以及刪除資產時的外鍵連動。
CREATE INDEX IF NOT EXISTS idx_interfaces_asset_id ON interfaces(asset_id);
-- 全系統重複 MAC 比對（僅提示、不阻擋，見 spec §3.2）。
CREATE INDEX IF NOT EXISTS idx_interfaces_mac ON interfaces(mac);
