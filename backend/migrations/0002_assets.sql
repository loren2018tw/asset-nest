-- 票 01：資產管理。assets 資料表（見 spec §2.1、§2.5）。
CREATE TABLE IF NOT EXISTS assets (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    property_no    TEXT,
    description    TEXT NOT NULL CHECK (trim(description) <> ''),
    location       TEXT NOT NULL CHECK (trim(location) <> ''),
    device_serial  TEXT,
    brand          TEXT,
    model          TEXT,
    purchase_date  TEXT,
    lifespan_years INTEGER CHECK (lifespan_years IS NULL OR lifespan_years >= 0),
    note           TEXT,
    tags           TEXT NOT NULL DEFAULT '[]',
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- 搜尋為 LIKE 子字串掃描；篩選（位置／廠牌／設備序號）走不分大小寫索引。
CREATE INDEX IF NOT EXISTS idx_assets_location ON assets(location COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_assets_brand ON assets(brand COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_assets_device_serial ON assets(device_serial COLLATE NOCASE);
