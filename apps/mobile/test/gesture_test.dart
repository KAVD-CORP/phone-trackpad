import 'package:flutter_test/flutter_test.dart';
import 'package:trackpad_mobile/gesture.dart';

/// Table-driven gesture tests: recorded pointer-event sequences in,
/// expected outputs out. Times are caller milliseconds.

/// Feed a scripted sequence into [engine]. Each step is
/// `[kind, id, x, y, t]` with kind in {down, move, up, tick}.
/// Moves carry deltas in (x, y).
List<GestureOut> run(GestureEngine engine, List<List<Object>> steps) {
  final out = <GestureOut>[];
  for (final s in steps) {
    final kind = s[0] as String;
    final id = s[1] as int;
    final x = (s[2] as num).toDouble();
    final y = (s[3] as num).toDouble();
    final t = s[4] as int;
    switch (kind) {
      case 'down':
        out.addAll(engine.pointerDown(id, x, y, t));
      case 'move':
        out.addAll(engine.pointerMove(id, x, y, t));
      case 'up':
        out.addAll(engine.pointerUp(id, x, y, t));
      case 'tick':
        out.addAll(engine.tick(t));
    }
  }
  return out;
}

void main() {
  test('single tap emits left click', () {
    final e = GestureEngine();
    expect(
      run(e, [
        ['down', 1, 100, 100, 0],
        ['up', 1, 101, 101, 80],
      ]),
      [const OutClick(buttonLeft)],
    );
  });

  test('slow tap with big movement is not a click', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['move', 1, 30, 0, 16],
      ['up', 1, 130, 100, 300],
    ]);
    expect(out.whereType<OutClick>(), isEmpty);
    expect(out.whereType<OutMove>(), isNotEmpty);
  });

  test('slow move uses the precision (low) gain', () {
    final e = GestureEngine();
    // 2 px in 100 ms = 20 px/s -> gain 0.7 -> 1.4 px -> emits 1, keeps .4.
    final out = run(e, [
      ['down', 1, 0, 0, 0],
      ['move', 1, 2, 0, 100],
      ['move', 1, 2, 0, 200],
    ]);
    expect(out[0], const OutMove(1, 0));
    expect(out[1], const OutMove(1, 0));
  });

  test('fast move uses the high gain', () {
    final e = GestureEngine();
    // 100 px in 10 ms = 10000 px/s -> clamped top gain 2.0 -> 200 px.
    final out = run(e, [
      ['down', 1, 0, 0, 0],
      ['move', 1, 100, 0, 10],
    ]);
    expect(out, [const OutMove(200, 0)]);
  });

  test('gain curve is monotonic and bounded', () {
    expect(pointerGain(0, 1.0, true), 0.7);
    expect(pointerGain(150, 1.0, true), 0.7);
    final mid = pointerGain(1500, 1.0, true);
    expect(mid, greaterThan(0.7));
    expect(mid, lessThan(2.0));
    expect(pointerGain(1e6, 1.0, true), 2.0);
    expect(pointerGain(1e6, 1.0, false), 1.0);
    expect(pointerGain(1e6, 2.0, false), 2.0);
  });

  test('two-finger tap emits right click only', () {
    final e = GestureEngine();
    expect(
      run(e, [
        ['down', 1, 100, 100, 0],
        ['down', 2, 140, 100, 30],
        ['up', 1, 100, 100, 90],
        ['up', 2, 140, 100, 110],
      ]),
      [const OutClick(buttonRight)],
    );
  });

  test('three-finger tap emits middle click only', () {
    final e = GestureEngine();
    expect(
      run(e, [
        ['down', 1, 100, 100, 0],
        ['down', 2, 140, 100, 25],
        ['down', 3, 180, 100, 55],
        ['up', 1, 100, 100, 90],
        ['up', 2, 140, 100, 105],
        ['up', 3, 180, 100, 120],
      ]),
      [const OutClick(buttonMiddle)],
    );
  });

  test('slow three-finger drag is NOT a middle tap', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['down', 2, 140, 100, 30],
      ['down', 3, 180, 100, 60],
      ['up', 1, 100, 100, 400],
      ['up', 2, 140, 100, 420],
      ['up', 3, 180, 100, 440],
    ]);
    expect(out.whereType<OutClick>(), isEmpty);
  });

  test('four-finger tap is ignored (palm rule keeps its drop behavior)', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['down', 2, 140, 100, 25],
      ['down', 3, 180, 100, 55],
      ['down', 4, 220, 100, 80],
      ['up', 1, 100, 100, 110],
      ['up', 2, 140, 100, 125],
      ['up', 3, 180, 100, 140],
      ['up', 4, 220, 100, 155],
    ]);
    expect(out, isEmpty);
  });

  test('two-finger drag emits scroll, natural by default', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['down', 2, 140, 100, 30],
      ['move', 1, 0, 40, 50],
      ['move', 2, 0, 40, 60],
      ['up', 1, 100, 140, 200],
      ['up', 2, 140, 140, 210],
    ]);
    expect(out.whereType<OutMove>(), isEmpty);
    expect(out.whereType<OutClick>(), isEmpty);
    final scrolls = out.whereType<OutScroll>().toList();
    expect(scrolls, isNotEmpty);
    expect(
      scrolls.fold<int>(0, (a, s) => a + s.dy),
      greaterThan(0),
    );
  });

  test('inverted scroll flips the sign', () {
    final settings = TrackpadSettings(natural: false);
    final e = GestureEngine(settings);
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['down', 2, 140, 100, 30],
      ['move', 1, 0, 40, 50],
      ['move', 2, 0, 40, 60],
      ['up', 1, 100, 140, 200],
      ['up', 2, 140, 140, 210],
    ]);
    final total = out
        .whereType<OutScroll>()
        .fold<int>(0, (a, s) => a + s.dy);
    expect(total, lessThan(0));
  });

  test('finger added mid-move cancels move and scrolls without clicking', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['move', 1, 20, 0, 16],
      ['down', 2, 140, 100, 30],
      ['move', 1, 0, 40, 50],
      ['move', 2, 0, 40, 60],
      ['up', 1, 120, 140, 300],
      ['up', 2, 140, 140, 310],
    ]);
    expect(out.whereType<OutClick>(), isEmpty);
    expect(out.whereType<OutScroll>(), isNotEmpty);
  });

  test('tap-hold-drag emits down, moves, up in order', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['up', 1, 100, 100, 80], // tap: click
      ['down', 1, 101, 101, 200], // re-press inside double-tap window
      ['tick', 0, 0, 0, 500], // held past dragHoldMs
      ['move', 1, 30, 0, 520],
      ['up', 1, 131, 101, 600],
    ]);
    expect(out[0], const OutClick(buttonLeft));
    expect(out[1], const OutDown(buttonLeft));
    expect(out.last, const OutUp(buttonLeft));
    expect(out.whereType<OutMove>(), isNotEmpty);
  });

  test('second quick press without hold is a second click, not a drag', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['up', 1, 100, 100, 80],
      ['down', 1, 101, 101, 200],
      ['up', 1, 101, 101, 260],
    ]);
    expect(
      out,
      [const OutClick(buttonLeft), const OutClick(buttonLeft)],
    );
  });

  test('three fingers are ignored as palm until all lift', () {
    final e = GestureEngine();
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['down', 2, 140, 100, 10],
      ['down', 3, 180, 100, 20],
      ['move', 1, 50, 50, 30],
      ['up', 1, 150, 150, 40],
      ['up', 2, 140, 100, 50],
      ['up', 3, 180, 100, 60],
      // Fresh tap afterwards works again.
      ['down', 1, 100, 100, 500],
      ['up', 1, 100, 100, 560],
    ]);
    expect(out, [const OutClick(buttonLeft)]);
  });

  test('drag lock keeps the button held until the next tap', () {
    final e = GestureEngine(TrackpadSettings(dragLock: true));
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['up', 1, 100, 100, 80],
      ['down', 1, 101, 101, 200],
      ['tick', 0, 0, 0, 500],
      ['move', 1, 30, 0, 520],
      ['up', 1, 131, 101, 600], // no Up: still held
      ['down', 1, 200, 200, 1000], // release tap
      ['up', 1, 200, 200, 1060],
    ]);
    expect(out[0], const OutClick(buttonLeft));
    expect(out[1], const OutDown(buttonLeft));
    expect(out.whereType<OutUp>(), [const OutUp(buttonLeft)]);
    expect(out.whereType<OutClick>(), [const OutClick(buttonLeft)]);
  });

  test('tap-to-click off disables taps but not moves', () {
    final e = GestureEngine(TrackpadSettings(tapToClick: false));
    final out = run(e, [
      ['down', 1, 100, 100, 0],
      ['up', 1, 100, 100, 80],
      ['down', 1, 100, 100, 500],
      ['move', 1, 20, 0, 516],
      ['up', 1, 120, 100, 600],
    ]);
    expect(out.whereType<OutClick>(), isEmpty);
    expect(out.whereType<OutMove>(), isNotEmpty);
  });

  test('sensitivity scales moves', () {
    final e = GestureEngine(TrackpadSettings(sensitivity: 2.0, accel: false));
    final out = run(e, [
      ['down', 1, 0, 0, 0],
      ['move', 1, 10, 0, 16],
    ]);
    expect(out, [const OutMove(20, 0)]);
  });
}
