-- 票 01：排除範圍。subnet_exclusions 資料表（見 spec §3、ADR-0020）。
-- 排除範圍僅支援 IPv4（應用層檢查）；與 DHCP 位址池結構互斥。
CREATE TABLE IF NOT EXISTS subnet_exclusions (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    subnet_id INTEGER NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    start_ip  TEXT NOT NULL,
    end_ip    TEXT NOT NULL,
    note      TEXT
);

-- 依網段取排除範圍，以及刪除網段時的連動刪除。
CREATE INDEX IF NOT EXISTS idx_subnet_exclusions_subnet_id
    ON subnet_exclusions(subnet_id);
