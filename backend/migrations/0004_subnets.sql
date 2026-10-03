-- 票 03：網段設定。subnets 與 subnet_pools 資料表（見 spec §2.3、§2.5）。
-- 單一網段為單一地址族；pool 與 kea_subnet_id 僅支援 IPv4（應用層檢查）。
CREATE TABLE IF NOT EXISTS subnets (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    cidr          TEXT NOT NULL UNIQUE CHECK (trim(cidr) <> ''),
    name          TEXT,
    note          TEXT,
    gateway       TEXT,
    kea_subnet_id INTEGER UNIQUE,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS subnet_pools (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    subnet_id INTEGER NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    start_ip  TEXT NOT NULL,
    end_ip    TEXT NOT NULL
);

-- 依網段取 pools，以及刪除網段時的連動刪除。
CREATE INDEX IF NOT EXISTS idx_subnet_pools_subnet_id ON subnet_pools(subnet_id);
