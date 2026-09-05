export function fixedInput(value: string, decimals = 6): bigint {
  if (!/^-?\d+(\.\d+)?$/.test(value.trim()))
    throw new Error("Enter a valid decimal number.");
  const negative = value.trim().startsWith("-");
  const [whole, fraction = ""] = value.trim().replace("-", "").split(".");
  if (fraction.length > decimals)
    throw new Error(`Use at most ${decimals} decimal places.`);
  const result =
    BigInt(whole) * 10n ** BigInt(decimals) +
    BigInt(fraction.padEnd(decimals, "0") || "0");
  return negative ? -result : result;
}
export function fixed(value: bigint, decimals = 6, shown = 2): string {
  const negative = value < 0n,
    magnitude = negative ? -value : value;
  const scale = 10n ** BigInt(decimals),
    rounding = 10n ** BigInt(Math.max(0, decimals - shown));
  const rounded = ((magnitude + rounding / 2n) / rounding) * rounding;
  return `${negative ? "-" : ""}${(rounded / scale).toLocaleString()}${shown ? "." + (rounded % scale).toString().padStart(decimals, "0").slice(0, shown) : ""}`;
}
