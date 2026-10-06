import type { Motion } from "./wire.ts";

// Host cursor bookkeeping. dx/dy on the packet are totals since the session
// started, not the movement since the previous datagram. The host applies
// only the gap from the last accepted totals, so a lost packet is caught up
// by the next one and a late packet is ignored.

export type CursorState = {
  hasPacket: boolean;
  lastSeq: number;
  lastX: number;
  lastY: number;
};

export function emptyCursor(): CursorState {
  return { hasPacket: false, lastSeq: 0, lastX: 0, lastY: 0 };
}

export function applyMotion(
  state: CursorState,
  packet: Motion,
): { state: CursorState; dx: number; dy: number } | null {
  if (!state.hasPacket) {
    return {
      state: {
        hasPacket: true,
        lastSeq: packet.seq,
        lastX: packet.dx,
        lastY: packet.dy,
      },
      dx: packet.dx | 0,
      dy: packet.dy | 0,
    };
  }
  const seqGap = (packet.seq - state.lastSeq) | 0;
  if (seqGap <= 0) return null;
  return {
    state: {
      hasPacket: true,
      lastSeq: packet.seq,
      lastX: packet.dx,
      lastY: packet.dy,
    },
    dx: (packet.dx - state.lastX) | 0,
    dy: (packet.dy - state.lastY) | 0,
  };
}
