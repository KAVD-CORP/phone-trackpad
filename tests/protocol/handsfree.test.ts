import assert from "node:assert/strict";
import test from "node:test";
import {
  decodeHandsfreePlain,
  encodeHandsfreePlain,
  HandsfreeKind,
} from "../../src/protocol/handsfree.ts";
import { decodeMotion } from "../../src/protocol/wire.ts";

test("handsfree cleartext matches the rust golden vector", () => {
  const bytes = encodeHandsfreePlain({
    session: 0x0102030405060708n,
    seq: 7n,
    kind: HandsfreeKind.Command,
    payload: Uint8Array.from([0x68, 0x69]),
  });
  assert.ok(bytes);
  assert.deepEqual(
    [...bytes],
    [
      0x48, 0x46, 0x01, 0x02, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01,
      0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x68, 0x69,
    ],
  );
  const decoded = decodeHandsfreePlain(bytes);
  assert.ok(decoded);
  assert.equal(decoded.session, 0x0102030405060708n);
  assert.equal(decoded.seq, 7n);
  assert.equal(decoded.kind, HandsfreeKind.Command);
  assert.deepEqual([...decoded.payload], [0x68, 0x69]);
});

test("handsfree rejects session 0, empty cancel payload is the only empty kind, and it is not a motion packet", () => {
  assert.equal(
    encodeHandsfreePlain({
      session: 0n,
      seq: 1n,
      kind: HandsfreeKind.Cancel,
      payload: new Uint8Array(),
    }),
    null,
  );
  const cancel = encodeHandsfreePlain({
    session: 9n,
    seq: 1n,
    kind: HandsfreeKind.Cancel,
    payload: new Uint8Array(),
  });
  assert.ok(cancel);
  assert.equal(cancel.length, 20);
  assert.equal(decodeMotion(cancel), null);
  const dirty = Uint8Array.from(cancel);
  dirty[3] = 9;
  assert.equal(decodeHandsfreePlain(dirty), null);
});
