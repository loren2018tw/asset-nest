/**
 * 前端 CIDR 工具：供網段對話框即時顯示結構錯誤（見 spec §4.2）。
 *
 * 後端仍是結構驗證的權威；此處解析失敗一律回傳 `null`（不猜測），
 * 交由後端回報格式錯誤。
 */

export type AddressFamily = "ipv4" | "ipv6";

/** 解析後的單一位址；`value` 為 v4（32 位）或 v6（128 位）數值。 */
export interface ParsedAddress {
  family: AddressFamily;
  value: bigint;
}

/** 解析後的 CIDR。 */
export interface ParsedCidr extends ParsedAddress {
  /** 前綴長度（位元）。 */
  prefix: number;
}

const IPV4_BITS = 32;
const IPV6_BITS = 128;

/** 地址族對應的位元數。 */
function bitsOf(family: AddressFamily): number {
  return family === "ipv4" ? IPV4_BITS : IPV6_BITS;
}

/** 解析單一位址（不含前綴）；不合法回傳 `null`。 */
export function parseAddress(text: string): ParsedAddress | null {
  const value = text.trim();
  if (value.includes(":")) {
    const parsed = parseIpv6(value);
    return parsed === null ? null : { family: "ipv6", value: parsed };
  }
  const parsed = parseIpv4(value);
  return parsed === null ? null : { family: "ipv4", value: parsed };
}

/** 解析 CIDR；不合法回傳 `null`。host bits 不收斂（僅比較範圍用）。 */
export function parseCidr(text: string): ParsedCidr | null {
  const value = text.trim();
  const slash = value.indexOf("/");
  if (slash <= 0 || slash === value.length - 1) {
    return null;
  }

  const address = parseAddress(value.slice(0, slash));
  if (address === null) {
    return null;
  }

  const prefixText = value.slice(slash + 1);
  if (!/^\d+$/.test(prefixText)) {
    return null;
  }

  const prefix = Number(prefixText);
  if (prefix > bitsOf(address.family)) {
    return null;
  }

  return { ...address, prefix };
}

/** 兩個 CIDR 是否重疊（含完全相同與嵌套）。 */
export function cidrsOverlap(a: ParsedCidr, b: ParsedCidr): boolean {
  if (a.family !== b.family) {
    return false;
  }
  const mask = prefixMask(Math.min(a.prefix, b.prefix), bitsOf(a.family));
  return (a.value & mask) === (b.value & mask);
}

/** 位址是否落在 CIDR 內（含 network 與 broadcast 位址）。 */
export function cidrContainsAddress(
  cidr: ParsedCidr,
  address: ParsedAddress
): boolean {
  if (cidr.family !== address.family) {
    return false;
  }
  const mask = prefixMask(cidr.prefix, bitsOf(cidr.family));
  return (cidr.value & mask) === (address.value & mask);
}

/** 前綴遮罩：前 `prefix` 位為 1、其餘為 0。 */
function prefixMask(prefix: number, bits: number): bigint {
  return ((1n << BigInt(prefix)) - 1n) << BigInt(bits - prefix);
}

/** IPv4：四段十進位、每段 0–255。 */
function parseIpv4(text: string): bigint | null {
  const octets = text.split(".");
  if (octets.length !== 4) {
    return null;
  }

  let value = 0n;
  for (const octet of octets) {
    if (!/^\d{1,3}$/.test(octet)) {
      return null;
    }
    const number = Number(octet);
    if (number > 255) {
      return null;
    }
    value = (value << 8n) | BigInt(number);
  }
  return value;
}

/** IPv6：支援 `::` 省略與末段內嵌 IPv4（如 `::ffff:192.168.1.1`）。 */
function parseIpv6(text: string): bigint | null {
  const halves = text.split("::");
  if (halves.length > 2) {
    return null;
  }

  const left = parseIpv6Groups(halves[0] ?? "", halves.length === 1);
  const right = parseIpv6Groups(halves[1] ?? "", halves.length === 2);
  if (left === null || right === null) {
    return null;
  }

  const given = left.length + right.length;
  if (halves.length === 1) {
    if (given !== 8) {
      return null;
    }
  } else if (given > 7) {
    return null;
  }

  const zeros = halves.length === 2 ? 8 - given : 0;
  const groups = [...left, ...Array.from({ length: zeros }, () => 0), ...right];

  let value = 0n;
  for (const group of groups) {
    value = (value << 16n) | BigInt(group);
  }
  return value;
}

/**
 * 解析 `::` 單側的 IPv6 群組；`allowEmbedded` 允許末群組為內嵌 IPv4
 * （僅整段位址的結尾合法）。
 */
function parseIpv6Groups(
  text: string,
  allowEmbedded: boolean
): number[] | null {
  if (text === "") {
    return [];
  }

  const parts = text.split(":");
  const groups: number[] = [];
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index] ?? "";
    if (allowEmbedded && index === parts.length - 1 && part.includes(".")) {
      const embedded = parseIpv4(part);
      if (embedded === null) {
        return null;
      }
      groups.push(
        Number((embedded >> 16n) & 0xffffn),
        Number(embedded & 0xffffn)
      );
      continue;
    }
    if (!/^[0-9a-fA-F]{1,4}$/.test(part)) {
      return null;
    }
    groups.push(Number.parseInt(part, 16));
  }
  return groups;
}
