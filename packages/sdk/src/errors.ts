/**
 * The program's custom error codes, in the order `program/src/error.rs` declares them.
 *
 * A simulation that comes back as `custom program error: 0x14` is useless on its own. Every code
 * here maps to the sentence a keeper or a bidder needs to decide what to do next.
 */
export const FLOCK_ERRORS: Record<number, string> = {
  0: "Account is not owned by the index program, or is not an index account.",
  1: "A derived address did not match the account that was passed.",
  2: "A required signature was missing.",
  3: "Only the governor can do that.",
  4: "Only the manager can do that.",
  5: "The index is sealed and cannot take new components.",
  6: "The index has not been sealed yet.",
  7: "Issuance and bidding are paused. Redemption is still open.",
  8: "The index already holds the maximum number of components.",
  9: "That mint is already a component of this index.",
  10: "No component at that position.",
  11: "A token account has the wrong mint or the wrong owner.",
  12: "The index mint must have 9 decimals, no freeze authority, and the index PDA as mint authority.",
  13: "That fee exceeds the cap the program enforces.",
  14: "Arithmetic overflow.",
  15: "Amount must be greater than zero.",
  16: "Your slippage bound was crossed: the recipe costs more, or pays less, than you allowed.",
  17: "A rebalance auction is already running.",
  18: "No rebalance auction is running.",
  19: "The auction has not opened yet, or has already closed.",
  20: "That component is already at or below its target: there is nothing to sell.",
  21: "The bid would take fund NAV below the floor the proposal committed to.",
  22: "Auction premiums are inverted or exceed the index's ceiling.",
  23: "Wrong number of entries: one is needed per component.",
  24: "A reference price of zero cannot value anything.",
  25: "The rebalance timelock has not elapsed.",
  26: "Supply is too small for that redemption.",
  27: "The proposal is stale.",
  28: "A bid must move two different components.",
  29: "The bid would sell more than the fund holds above target.",
  30: "That component is already at the cap the auction may raise it to.",
  31: "The payment would push that component past its cap.",
};

/** Turn a thrown `SendTransactionError` into the sentence behind its code, when there is one. */
export function explainError(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  const match = /custom program error: 0x([0-9a-fA-F]+)/.exec(text) ?? /Custom\((\d+)\)/.exec(text);
  if (!match?.[1]) return text;
  const code = match[0].includes("0x") ? parseInt(match[1], 16) : Number(match[1]);
  const known = FLOCK_ERRORS[code];
  return known ? `${known} (flock error ${code})` : text;
}
