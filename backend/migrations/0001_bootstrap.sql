-- 骨架階段基礎遷移：僅建立中繼資料表，尚無領域表。
CREATE TABLE IF NOT EXISTS app_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
