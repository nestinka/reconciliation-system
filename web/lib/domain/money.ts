/**
 * Format an amount expressed in minor units (integer) to a localized currency string.
 *
 * Money is always stored as integer minor units (e.g. 123456 GBP = £1,234.56).
 * The number of decimal places is derived from the currency via Intl.NumberFormat.
 */
export function formatMoney(amountMinor: number, currency: string): string {
  // Resolve how many fractional digits the currency uses (JPY=0, most others=2)
  const decimals =
    new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
    }).resolvedOptions().maximumFractionDigits ?? 2;

  const amount = amountMinor / Math.pow(10, decimals);

  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency,
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(amount);
}
