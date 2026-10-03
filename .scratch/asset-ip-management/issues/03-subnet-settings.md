# 03: 網段設定

**What to build:** 網段設定頁與 CRUD;v4/v6 單一地址族網段,含名稱、備註、gateway、pool 多段(v4)、Kea subnet-id(v4)。結構性驗證阻擋非法資料。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 可建立、編輯、刪除網段;CIDR 為必填且單位址族
- [ ] v4 可設多段 pool 與 kea_subnet_id(唯一);v6 不接受 pool 與 kea_subnet_id
- [ ] 結構驗證阻擋並有明確錯誤:與既有網段重疊(含完全相同、嵌套)、gateway 不在 CIDR 內、pool 不在 CIDR 內、pool 段間重疊
- [ ] 網段名稱選填、不強制唯一
- [ ] 網段列表顯示名稱、CIDR 與地址族
- [ ] 後端整合測試涵蓋重疊與各項結構驗證
