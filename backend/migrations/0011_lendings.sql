-- 票 01：資產借還。lendings 資料表（見 spec §3）。
-- 一筆「借出到歸還」的完整紀錄；returned_at 為 NULL 即出借中。
CREATE TABLE IF NOT EXISTS lendings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id    INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    borrower    TEXT    NOT NULL,
    lent_at     TEXT    NOT NULL,
    due_at      TEXT,
    note        TEXT,
    returned_at TEXT
);

-- 依資產取借還紀錄（出借中檢查），以及刪除資產時的連動刪除（FK CASCADE）。
CREATE INDEX IF NOT EXISTS idx_lendings_asset_id ON lendings(asset_id);
-- 出借中／已歸還清單依 returned_at 是否為 NULL 區分。
CREATE INDEX IF NOT EXISTS idx_lendings_returned ON lendings(returned_at);
