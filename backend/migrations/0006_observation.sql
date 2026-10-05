-- 票 01：IP 觀測（見 spec §資料庫（migration 0006）、ADR-0014、ADR-0016）。
-- subnets 新增觀測開關欄位；觀測資料獨立成兩表，永不修改宣告資料。
ALTER TABLE subnets ADD COLUMN observed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subnets ADD COLUMN discovery_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subnets ADD COLUMN discovery_interval_minutes INTEGER NULL;
ALTER TABLE subnets ADD COLUMN last_discovery_at TEXT NULL;

-- 現況：每個 (subnet_id, address) 一列，主鍵對齊 ip_assignments；
-- 現況列由掃描產生，取消指派不刪列（歷史保留）。
CREATE TABLE IF NOT EXISTS ip_presence (
    subnet_id        INTEGER NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    address          TEXT NOT NULL CHECK (trim(address) <> ''),
    last_seen_at     TEXT,
    last_seen_mac    TEXT,
    last_seen_source TEXT,
    last_checked_at  TEXT,
    PRIMARY KEY (subnet_id, address)
);

-- 事件：append-only，只在變化時寫（first_seen／mac_changed）。
CREATE TABLE IF NOT EXISTS observation_event (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    subnet_id   INTEGER NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    address     TEXT NOT NULL CHECK (trim(address) <> ''),
    mac         TEXT,
    kind        TEXT NOT NULL CHECK (kind IN ('first_seen', 'mac_changed')),
    source      TEXT NOT NULL,
    observed_at TEXT NOT NULL
);

-- 位址視角的歷史查詢、MAC 視角查詢與保留期清理。
CREATE INDEX IF NOT EXISTS idx_observation_event_subnet_address_observed
    ON observation_event(subnet_id, address, observed_at);
CREATE INDEX IF NOT EXISTS idx_observation_event_mac_observed
    ON observation_event(mac, observed_at);
CREATE INDEX IF NOT EXISTS idx_observation_event_observed
    ON observation_event(observed_at);
