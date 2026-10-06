-- 票 08：觀測執行外移給代理後，網段層觀測設定退場（見 ADR-0018）。
-- 移除觀測開關、探索開關／間隔與上次探索時間；「已觀測」改由在線代理涵蓋判定。
ALTER TABLE subnets DROP COLUMN observed;
ALTER TABLE subnets DROP COLUMN discovery_enabled;
ALTER TABLE subnets DROP COLUMN discovery_interval_minutes;
ALTER TABLE subnets DROP COLUMN last_discovery_at;
