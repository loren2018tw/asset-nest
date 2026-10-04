/**
 * Vite `?raw` 匯入的型別宣告（見 https://vite.dev/guide/assets#importing-asset-as-string）。
 * 本專案未使用 `vite/client` 型別，故在此補上。
 */
declare module "*?raw" {
  const content: string;
  export default content;
}
