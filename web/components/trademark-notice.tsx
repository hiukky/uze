// Nominative use: uze names these harnesses because it interoperates with
// them, which is exactly the claim this notice keeps narrow. Deliberately
// names none of them — the supported set is meant to grow, and a list that
// goes stale reads as excluding whatever is missing from it. It renders once,
// in the home page footer: repeating it under every docs page gave a
// housekeeping note more weight than the documentation it sat under.
export function TrademarkNotice() {
  return (
    <p className="mx-auto max-w-[70ch] text-[11px] leading-relaxed text-muted">
      All product names, logos and brands are the property of their respective owners. uze names the
      coding agents it interoperates with for identification only, and is not affiliated with,
      endorsed by, or sponsored by any of them.
    </p>
  );
}
