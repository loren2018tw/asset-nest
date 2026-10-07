import { defineStore } from "pinia";

import {
  fetchSession,
  login as apiLogin,
  logout as apiLogout
} from "@/api/auth";

/**
 * 工作階段狀態（見 spec §5.1）。
 *
 * `checked` 代表已向後端確認過一次登入狀態，守衛只在首次導覽探測；
 * 之後的工作階段失效一律由全域 401 攔截呼叫 `expire()` 處理。
 */
export const useAuthStore = defineStore("auth", {
  state: () => ({
    /** 目前登入帳號；`null`＝未登入。 */
    username: null as string | null,
    /** 是否已向後端確認過登入狀態。 */
    checked: false
  }),

  actions: {
    /** 首次確認工作階段；僅查一次。401 或網路失敗一律視同未登入。 */
    async ensureSession(): Promise<void> {
      if (this.checked) {
        return;
      }

      try {
        const session = await fetchSession();
        this.username = session.username;
      } catch {
        // 含 401（未登入）與其他失敗：標記未登入，避免守衛卡住
        this.username = null;
      }

      this.checked = true;
    },

    /** 登入：成功設定帳號與已確認狀態；失敗原樣拋出，由登入頁顯示訊息。 */
    async login(username: string, password: string): Promise<void> {
      const session = await apiLogin(username, password);
      this.username = session.username;
      this.checked = true;
    },

    /** 登出：API 失敗時 rethrow（維持登入狀態）；成功清除帳號。 */
    async logout(): Promise<void> {
      await apiLogout();
      this.username = null;
    },

    /** 工作階段過期（401 攔截用）：清除帳號；`checked` 維持 true 不再探測。 */
    expire(): void {
      this.username = null;
    }
  }
});
