-- 票 01：觀測代理心跳與狀態（見 specs §資料庫（migration 0008）、ADR-0019）。
-- 只新增表，不動既有表；subnets 觀測欄位移除見票 08。
CREATE TABLE IF NOT EXISTS agent (
    -- 代理自報的 instance_id。
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    version             TEXT NOT NULL,
    -- 連線來源；忽略 X-Forwarded-For（見 ADR-0019）。
    source_ip           TEXT NOT NULL,
    -- 代理回報並以 ipnet 正規化的涵蓋 CIDR。
    subnet_cidr         TEXT NOT NULL,
    -- 精確對應的受管網段；網段刪除時設空（代理仍可見，標「未對應」）。
    subnet_id           INTEGER REFERENCES subnets(id) ON DELETE SET NULL,
    first_report_at     TEXT NOT NULL,
    last_report_at      TEXT NOT NULL,
    last_observation_at TEXT
);

-- 代理清單以對應網段查詢／顯示用。
CREATE INDEX IF NOT EXISTS idx_agent_subnet_id ON agent(subnet_id);

-- 認證碼不符的回報：以來源 IP 彙總首末時間與次數；自報名稱／版本 best-effort。
CREATE TABLE IF NOT EXISTS agent_auth_failure (
    source_ip        TEXT PRIMARY KEY,
    claimed_name     TEXT,
    claimed_version  TEXT,
    first_attempt_at TEXT NOT NULL,
    last_attempt_at  TEXT NOT NULL,
    attempt_count    INTEGER NOT NULL DEFAULT 1
);
