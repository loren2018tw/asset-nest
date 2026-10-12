-- 票 01：資產報廢。disposed_assets 資料表（見 spec §3、ADR-0026）。
-- 報廢＝整列複製到此表＋報廢日期後自 assets 刪除；復原＝搬回後刪除此列。
CREATE TABLE IF NOT EXISTS disposed_assets (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id       INTEGER NOT NULL UNIQUE,
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
    disposal_date  TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

-- 報廢清單依報廢日期新→舊排序。
CREATE INDEX IF NOT EXISTS idx_disposed_assets_disposal_date ON disposed_assets(disposal_date);
