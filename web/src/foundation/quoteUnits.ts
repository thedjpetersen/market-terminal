// Provider units are case-sensitive. Formatting never changes an economic value.
const major = new Set([
  "USD",
  "GBP",
  "EUR",
  "JPY",
  "CAD",
  "AUD",
  "CHF",
  "CNY",
  "HKD",
  "SEK",
  "NOK",
  "DKK",
  "NZD",
  "INR",
  "KRW",
  "BRL",
  "ZAR",
  "ILS",
]);
export function formatQuotePrice(value: number, unit: string): string {
  const amount = new Intl.NumberFormat(undefined, {
    maximumFractionDigits: 2,
  }).format(value);
  if (unit === "GBp" || unit === "GBX") return `${amount}p`;
  if (unit === "ZAc") return `${amount} ZAc`;
  if (major.has(unit))
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: unit,
      maximumFractionDigits: 2,
    }).format(value);
  return `${amount} ${unit || "UNKNOWN UNIT"}`;
}
