-- 票 05：IP 指派。ip_assignments 資料表（見 spec §2.5、ADR-0005）。
-- 指派對象是 Interface；同一網段同一位址僅一筆、同一介面同一網段至多一筆。
-- subnet 的 FK 不設連動：有指派的網段不可刪除（防護見票 08）；
-- interface 刪除則連動刪除指派。
CREATE TABLE IF NOT EXISTS ip_assignments (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    subnet_id    INTEGER NOT NULL REFERENCES subnets(id),
    address      TEXT NOT NULL CHECK (trim(address) <> ''),
    interface_id INTEGER NOT NULL REFERENCES interfaces(id) ON DELETE CASCADE,
    purpose      TEXT NOT NULL CHECK (purpose IN ('static', 'reservation')),
    hostname     TEXT,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (subnet_id, address),
    UNIQUE (interface_id, subnet_id)
);
