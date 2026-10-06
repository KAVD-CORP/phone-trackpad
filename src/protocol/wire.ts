// Byte oracle for Phone Trackpad. Dart and Rust ports must match these bytes.
// Motion totals are cumulative since the session started. A lost datagram is
// recovered when a later one arrives. See docs/PROTOCOL.md.

export const VERSION = 1;

export const MOTION_LENGTH = 20;
export const ACK_LENGTH = 12;
export const CONTROL_LENGTH = 18;

export const Kind = {
  LeftClick: 1,
  RightClick: 2,
  ButtonDown: 3,
  ButtonUp: 4,
  Scroll: 5,
} as const;

export type Kind = (typeof Kind)[keyof typeof Kind];

export type Motion = {
  session: number;
  seq: number;
  dx: number;
  dy: number;
};

export type Ack = {
  session: number;
  ackSeq: number;
};

export type Control = {
  session: number;
  seq: number;
  kind: Kind;
  button: number;
  dx: number;
  dy: number;
};

const KIND_OK = new Set<number>(Object.values(Kind));

function u32(n: number): number {
  return n >>> 0;
}

function i32(n: number): number {
  return n | 0;
}

function i16(n: number): number {
  const v = n | 0;
  return ((v << 16) >> 16);
}

export function encodeMotion(packet: Motion): Uint8Array {
  const out = new Uint8Array(MOTION_LENGTH);
  const view = new DataView(out.buffer);
  out[0] = 0x54;
  out[1] = 0x50;
  out[2] = VERSION;
  out[3] = 0;
  view.setUint32(4, u32(packet.session), true);
  view.setUint32(8, u32(packet.seq), true);
  view.setInt32(12, i32(packet.dx), true);
  view.setInt32(16, i32(packet.dy), true);
  return out;
}

export function decodeMotion(bytes: Uint8Array): Motion | null {
  if (bytes.length !== MOTION_LENGTH) return null;
  if (bytes[0] !== 0x54 || bytes[1] !== 0x50) return null;
  if (bytes[2] !== VERSION) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const session = view.getUint32(4, true);
  if (session === 0) return null;
  return {
    session,
    seq: view.getUint32(8, true),
    dx: view.getInt32(12, true),
    dy: view.getInt32(16, true),
  };
}

export function encodeAck(packet: Ack): Uint8Array {
  const out = new Uint8Array(ACK_LENGTH);
  const view = new DataView(out.buffer);
  out[0] = 0x54;
  out[1] = 0x41;
  out[2] = VERSION;
  out[3] = 0;
  view.setUint32(4, u32(packet.session), true);
  view.setUint32(8, u32(packet.ackSeq), true);
  return out;
}

export function decodeAck(bytes: Uint8Array): Ack | null {
  if (bytes.length !== ACK_LENGTH) return null;
  if (bytes[0] !== 0x54 || bytes[1] !== 0x41) return null;
  if (bytes[2] !== VERSION) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const session = view.getUint32(4, true);
  if (session === 0) return null;
  return { session, ackSeq: view.getUint32(8, true) };
}

export function encodeControl(packet: Control): Uint8Array | null {
  if (!KIND_OK.has(packet.kind)) return null;
  const out = new Uint8Array(CONTROL_LENGTH);
  const view = new DataView(out.buffer);
  out[0] = 0x54;
  out[1] = 0x43;
  out[2] = VERSION;
  out[3] = packet.kind;
  view.setUint32(4, u32(packet.session), true);
  view.setUint32(8, u32(packet.seq), true);
  out[12] = packet.button & 0xff;
  out[13] = 0;
  view.setInt16(14, i16(packet.dx), true);
  view.setInt16(16, i16(packet.dy), true);
  return out;
}

export function decodeControl(bytes: Uint8Array): Control | null {
  if (bytes.length !== CONTROL_LENGTH) return null;
  if (bytes[0] !== 0x54 || bytes[1] !== 0x43) return null;
  if (bytes[2] !== VERSION) return null;
  if (!KIND_OK.has(bytes[3])) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const session = view.getUint32(4, true);
  if (session === 0) return null;
  return {
    session,
    seq: view.getUint32(8, true),
    kind: bytes[3] as Kind,
    button: bytes[12],
    dx: view.getInt16(14, true),
    dy: view.getInt16(16, true),
  };
}
