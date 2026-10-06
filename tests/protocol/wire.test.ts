import assert from "node:assert/strict";
import test from "node:test";
import { applyMotion, emptyCursor } from "../../src/protocol/apply.ts";
import {
  decodeAck,
  decodeControl,
  decodeMotion,
  encodeAck,
  encodeControl,
  encodeMotion,
  Kind,
} from "../../src/protocol/wire.ts";

test("motion bytes are stable for the golden vector", () => {
  const bytes = encodeMotion({ session: 0x01020304, seq: 7, dx: 12, dy: -4 });
  assert.deepEqual(
    [...bytes],
    [
      0x54, 0x50, 0x01, 0x00, 0x04, 0x03, 0x02, 0x01, 0x07, 0x00, 0x00, 0x00,
      0x0c, 0x00, 0x00, 0x00, 0xfc, 0xff, 0xff, 0xff,
    ],
  );
  assert.deepEqual(decodeMotion(bytes), {
    session: 0x01020304,
    seq: 7,
    dx: 12,
    dy: -4,
  });
});

test("a later packet applies only the gap after a loss", () => {
  const first = applyMotion(
    emptyCursor(),
    { session: 1, seq: 1, dx: 10, dy: -4 },
  );
  assert.ok(first);
  assert.deepEqual({ dx: first.dx, dy: first.dy }, { dx: 10, dy: -4 });

  const caughtUp = applyMotion(first.state, {
    session: 1,
    seq: 3,
    dx: 18,
    dy: -6,
  });
  assert.ok(caughtUp);
  assert.deepEqual({ dx: caughtUp.dx, dy: caughtUp.dy }, { dx: 8, dy: -2 });
});

test("a stale motion packet does not move the cursor again", () => {
  const first = applyMotion(
    emptyCursor(),
    { session: 1, seq: 3, dx: 18, dy: -6 },
  );
  assert.ok(first);
  assert.equal(
    applyMotion(first.state, { session: 1, seq: 1, dx: 10, dy: -4 }),
    null,
  );
});

test("totals wrap as int32", () => {
  const first = applyMotion(emptyCursor(), {
    session: 1,
    seq: 1,
    dx: 2147483640,
    dy: 0,
  });
  assert.ok(first);
  const next = applyMotion(first.state, {
    session: 1,
    seq: 2,
    dx: -2147483640,
    dy: 0,
  });
  assert.ok(next);
  assert.equal(next.dx, 16);
});

test("bad magic, short buffer, and session 0 are dropped", () => {
  const ok = encodeMotion({ session: 1, seq: 1, dx: 1, dy: 1 });
  const bad = new Uint8Array(ok);
  bad[0] = 0x00;
  assert.equal(decodeMotion(bad), null);
  assert.equal(decodeMotion(ok.slice(0, 8)), null);
  assert.equal(decodeMotion(encodeMotion({ session: 0, seq: 1, dx: 1, dy: 1 })), null);
});

test("ack and control round-trip", () => {
  const ack = { session: 9, ackSeq: 4 };
  assert.deepEqual(decodeAck(encodeAck(ack)), ack);

  const click = {
    session: 9,
    seq: 4,
    kind: Kind.LeftClick,
    button: 0,
    dx: 0,
    dy: 0,
  };
  assert.deepEqual(decodeControl(encodeControl(click)!), click);

  const scroll = {
    session: 9,
    seq: 5,
    kind: Kind.Scroll,
    button: 0,
    dx: 0,
    dy: -8,
  };
  assert.deepEqual(decodeControl(encodeControl(scroll)!), scroll);
  assert.equal(encodeControl({ ...click, kind: 9 as Kind }), null);
});
