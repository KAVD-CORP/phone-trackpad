// Handsfree TCP control frame (cleartext, before ChaCha20-Poly1305).
// Motion packets are unchanged. Session 0 is rejected. See docs/PROTOCOL.md.

export const HANDSFREE_VERSION = 1;
export const HANDSFREE_HEADER_LENGTH = 20;
export const MAX_VOICE_LENGTH = 1_048_576;
export const MAX_TEXT_LENGTH = 8_192;

export const HandsfreeKind = {
  Voice: 1,
  Command: 2,
  TypeText: 3,
  KeyCombo: 4,
  Cancel: 5,
} as const;

export type HandsfreeKind = (typeof HandsfreeKind)[keyof typeof HandsfreeKind];

export type HandsfreePlain = {
  session: bigint;
  seq: bigint;
  kind: HandsfreeKind;
  payload: Uint8Array;
};

const LIMITS = new Map<number, number>([
  [HandsfreeKind.Voice, MAX_VOICE_LENGTH],
  [HandsfreeKind.Command, MAX_TEXT_LENGTH],
  [HandsfreeKind.TypeText, MAX_TEXT_LENGTH],
  [HandsfreeKind.KeyCombo, MAX_TEXT_LENGTH],
  [HandsfreeKind.Cancel, 0],
]);

function writeU64(view: DataView, offset: number, value: bigint): boolean {
  if (value < 0n || value > 0xffff_ffff_ffff_ffffn) return false;
  view.setBigUint64(offset, value, true);
  return true;
}

export function encodeHandsfreePlain(frame: HandsfreePlain): Uint8Array | null {
  if (frame.session === 0n || frame.seq === 0n) return null;
  const limit = LIMITS.get(frame.kind);
  if (limit === undefined || frame.payload.length > limit) return null;
  const out = new Uint8Array(HANDSFREE_HEADER_LENGTH + frame.payload.length);
  const view = new DataView(out.buffer);
  out[0] = 0x48;
  out[1] = 0x46;
  out[2] = HANDSFREE_VERSION;
  out[3] = frame.kind;
  if (!writeU64(view, 4, frame.session) || !writeU64(view, 12, frame.seq)) return null;
  out.set(frame.payload, HANDSFREE_HEADER_LENGTH);
  return out;
}

export function decodeHandsfreePlain(bytes: Uint8Array): HandsfreePlain | null {
  if (bytes.length < HANDSFREE_HEADER_LENGTH) return null;
  if (bytes[0] !== 0x48 || bytes[1] !== 0x46) return null;
  if (bytes[2] !== HANDSFREE_VERSION) return null;
  const limit = LIMITS.get(bytes[3]);
  if (limit === undefined) return null;
  const payload = bytes.subarray(HANDSFREE_HEADER_LENGTH);
  if (payload.length > limit) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const session = view.getBigUint64(4, true);
  const seq = view.getBigUint64(12, true);
  if (session === 0n || seq === 0n) return null;
  return {
    session,
    seq,
    kind: bytes[3] as HandsfreeKind,
    payload: Uint8Array.from(payload),
  };
}
