// IPv4 subnet calculator. Pure, for testing.

export interface SubnetInfo {
  network: string;
  broadcast: string;
  netmask: string;
  wildcard: string;
  prefix: number;
  firstHost: string;
  lastHost: string;
  hosts: number;
}

function parseIp(s: string): number | null {
  const parts = s.trim().split(".");
  if (parts.length !== 4) return null;
  let n = 0;
  for (const p of parts) {
    if (!/^\d{1,3}$/.test(p)) return null;
    const v = Number(p);
    if (v > 255) return null;
    n = n * 256 + v;
  }
  return n;
}

const fmt = (n: number) => [24, 16, 8, 0].map((s) => (n >>> s) & 255).join(".");

/** Accepts `10.0.0.5/24` or `10.0.0.5 255.255.255.0`. */
export function calcSubnet(input: string): SubnetInfo | null {
  const [ipPart, maskPart] = input.trim().split(/[\s/]+/);
  if (!ipPart || maskPart === undefined) return null;
  const ip = parseIp(ipPart);
  if (ip === null) return null;
  let prefix: number;
  if (/^\d{1,2}$/.test(maskPart)) {
    prefix = Number(maskPart);
    if (prefix > 32) return null;
  } else {
    const m = parseIp(maskPart);
    if (m === null) return null;
    const bits = m.toString(2).padStart(32, "0");
    if (!/^1*0*$/.test(bits)) return null; // non-contiguous mask
    prefix = bits.indexOf("0") === -1 ? 32 : bits.indexOf("0");
  }
  const mask = prefix === 0 ? 0 : (0xffffffff << (32 - prefix)) >>> 0;
  const network = (ip & mask) >>> 0;
  const broadcast = (network | (~mask >>> 0)) >>> 0;
  const special = prefix >= 31; // /31 point-to-point, /32 single host
  return {
    network: fmt(network),
    broadcast: fmt(broadcast),
    netmask: fmt(mask),
    wildcard: fmt(~mask >>> 0),
    prefix,
    firstHost: fmt(special ? network : network + 1),
    lastHost: fmt(special ? broadcast : broadcast - 1),
    hosts: prefix === 32 ? 1 : prefix === 31 ? 2 : 2 ** (32 - prefix) - 2,
  };
}
