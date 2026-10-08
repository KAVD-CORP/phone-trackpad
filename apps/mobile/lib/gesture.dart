/// Pure gesture state machine for the trackpad surface.
///
/// No Flutter imports: feed raw pointer data in, get wire-ready outputs out.
/// The widget layer (`main.dart`) translates `Listener` events into these
/// calls and forwards outputs to [TrackpadSender]. All timing uses caller
/// milliseconds so tests control time exactly.
///
/// Supported in Phase 2:
/// - one-finger drag → [OutMove] (sensitivity + velocity accel curve)
/// - quick tap → [OutClick] left (if [TrackpadSettings.tapToClick])
/// - two-finger drag → [OutScroll] (natural toggle applied here)
/// - two-finger tap → [OutClick] right
/// - three-finger tap → [OutClick] middle
///   (through [buttonMiddle]; open links in new tabs, close tabs, etc.)
/// - tap, re-press + hold/move, release → [OutDown]/[OutMove]/[OutUp]
///   (optional [TrackpadSettings.dragLock] keeps the button held)
///
/// Multi-touch transitions: a second finger mid-move cancels the one-finger
/// gesture and enters scroll mode (no jump: moves are delta-based).
/// Exactly three fingers is a tap candidate; four or more concurrent
/// touches is treated as palm/accident and ignored until all fingers lift.
/// Stationary edge presses never pass the tap travel threshold, so they
/// emit nothing.
library;

// Timing / distance thresholds. Exposed (not private) per spec so they can
// be tuned from measurements; device tuning lands in Phase 5.
const int tapMaxMs = 200;
const double tapMaxPx = 12.0;
const int doubleTapWindowMs = 350;
const double doubleTapMaxPx = 24.0;
const int dragHoldMs = 250;
const double dragSlopPx = 10.0;

// Pointer-acceleration curve (see [pointerGain]).
const double accelPrecisionSpeedPxS = 150.0;
const double accelFullSpeedPxS = 3000.0;
const double accelLowFactor = 0.7;
const double accelHighFactor = 2.0;

/// Left/right/middle button ids (mirror `trackpad-core` button ids).
const int buttonLeft = 0;
const int buttonRight = 1;
const int buttonMiddle = 2;

/// Exactly this many concurrent fingers enables the three-finger tap.
const int threeFingerTarget = 3;

/// User-facing pointer settings.
class TrackpadSettings {
  double sensitivity;
  bool accel;
  bool natural;
  bool tapToClick;
  bool dragLock;

  TrackpadSettings({
    this.sensitivity = 1.0,
    this.accel = true,
    this.natural = true,
    this.tapToClick = true,
    this.dragLock = false,
  });
}

/// Outputs. Integer deltas are wire-ready; sub-pixel remainders are already
/// carried inside the engine.
abstract class GestureOut {
  const GestureOut();
}

class OutMove extends GestureOut {
  final int dx;
  final int dy;
  const OutMove(this.dx, this.dy);

  @override
  bool operator ==(Object other) =>
      other is OutMove && other.dx == dx && other.dy == dy;
  @override
  int get hashCode => Object.hash(dx, dy);
  @override
  String toString() => 'Move($dx,$dy)';
}

class OutScroll extends GestureOut {
  final int dx;
  final int dy;
  const OutScroll(this.dx, this.dy);

  @override
  bool operator ==(Object other) =>
      other is OutScroll && other.dx == dx && other.dy == dy;
  @override
  int get hashCode => Object.hash(dx, dy);
  @override
  String toString() => 'Scroll($dx,$dy)';
}

class OutClick extends GestureOut {
  final int button;
  const OutClick(this.button);

  @override
  bool operator ==(Object other) => other is OutClick && other.button == button;
  @override
  int get hashCode => button;
  @override
  String toString() => 'Click($button)';
}

class OutDown extends GestureOut {
  final int button;
  const OutDown(this.button);

  @override
  bool operator ==(Object other) => other is OutDown && other.button == button;
  @override
  int get hashCode => button;
  @override
  String toString() => 'Down($button)';
}

class OutUp extends GestureOut {
  final int button;
  const OutUp(this.button);

  @override
  bool operator ==(Object other) => other is OutUp && other.button == button;
  @override
  int get hashCode => button;
  @override
  String toString() => 'Up($button)';
}

/// Velocity-based gain: gentle below [accelPrecisionSpeedPxS] for pixel
/// work, ramping to [accelHighFactor] at [accelFullSpeedPxS] for traversals.
/// Pure function so the curve itself is unit-testable.
double pointerGain(double speedPxS, double sensitivity, bool accel) {
  if (!accel) return sensitivity;
  if (speedPxS <= accelPrecisionSpeedPxS) return sensitivity * accelLowFactor;
  final t = ((speedPxS - accelPrecisionSpeedPxS) /
          (accelFullSpeedPxS - accelPrecisionSpeedPxS))
      .clamp(0.0, 1.0);
  return sensitivity * (accelLowFactor + (accelHighFactor - accelLowFactor) * t);
}

class _Touch {
  double x;
  double y;
  final int downT;
  _Touch(this.x, this.y, this.downT);
}

class GestureEngine {
  final TrackpadSettings settings;
  final Map<int, _Touch> _touches = {};

  int? _primary;
  int _downT = 0;
  double _downX = 0;
  double _downY = 0;
  double _travel = 0;
  int _lastMoveT = 0;
  double _remX = 0;
  double _remY = 0;

  int _lastTapT = -1000000;
  bool _maybeDrag = false;
  bool _dragging = false;
  bool _dragLockedHeld = false;

  bool _scrolling = false;
  int _twoDownT = 0;
  double _twoTravel = 0;
  double _scrollRemX = 0;
  double _scrollRemY = 0;

  // Three-finger tap tracking. Down time of the third finger and the
  // combined travel of ALL fingers since it landed decide tap success.
  bool _threeTap = false;
  int _threeDownT = 0;
  double _threeTravel = 0;

  bool _palm = false;

  /// Set when two fingers were down: the trailing single-finger lift must
  /// never count as a tap. Cleared on the next fresh single-finger down.
  bool _noTap = false;

  GestureEngine([TrackpadSettings? settings])
      : settings = settings ?? TrackpadSettings();

  List<GestureOut> pointerDown(int id, double x, double y, int t) {
    // Release a drag-locked button on the next tap anywhere.
    if (_dragLockedHeld && _touches.isEmpty) {
      _dragLockedHeld = false;
      _noTap = true; // the release tap itself must not click
      _touches[id] = _Touch(x, y, t);
      _primary = id;
      _downT = t;
      _travel = 0;
      _lastMoveT = t;
      return const [OutUp(buttonLeft)];
    }
    _touches[id] = _Touch(x, y, t);
    if (_touches.length > threeFingerTarget) {
      _palm = true;
      _maybeDrag = false;
      _scrolling = false;
      _threeTap = false;
      return const [];
    }
    if (_touches.length == threeFingerTarget) {
      // Exactly three fingers: tap candidate (middle click). Two-finger
      // scroll is cancelled like the one-finger case; all three lifts are
      // consumed by the tap logic so no stray click/scroll escapes.
      final out = <GestureOut>[];
      if (_dragging) {
        _dragging = false;
        out.add(const OutUp(buttonLeft));
      }
      _maybeDrag = false;
      _scrolling = false;
      _threeTap = true;
      _threeDownT = t;
      _threeTravel = 0;
      _noTap = true;
      return out;
    }
    if (_touches.length == 2) {
      // Second finger: cancel one-finger state, enter scroll mode.
      // An in-progress drag ends first so DOWN is never left hanging.
      final out = <GestureOut>[];
      if (_dragging) {
        _dragging = false;
        out.add(const OutUp(buttonLeft));
      }
      _maybeDrag = false;
      _scrolling = true;
      _twoDownT = t;
      _twoTravel = 0;
      _scrollRemX = 0;
      _scrollRemY = 0;
      _noTap = true;
      return out;
    }
    // First finger.
    _noTap = false;
    _maybeDrag = _lastTapT > 0 &&
        (t - _lastTapT) <= doubleTapWindowMs &&
        (x - _lastTapX).abs() <= doubleTapMaxPx &&
        (y - _lastTapY).abs() <= doubleTapMaxPx;
    _primary = id;
    _downT = t;
    _downX = x;
    _downY = y;
    _travel = 0;
    _lastMoveT = t;
    _remX = 0;
    _remY = 0;
    return const [];
  }

  List<GestureOut> pointerMove(int id, double dx, double dy, int t) {
    final touch = _touches[id];
    if (touch == null || _palm) return const [];
    touch.x += dx;
    touch.y += dy;
    if (_threeTap) {
      _threeTravel += dx.abs() + dy.abs();
      return const [];
    }
    if (_scrolling) {
      // Average the two fingers: per-finger deltas over-count by ~2x.
      _twoTravel += dx.abs() + dy.abs();
      final ax = dx / 2 + _scrollRemX;
      final ay = dy / 2 + _scrollRemY;
      final ix = ax.truncate();
      final iy = ay.truncate();
      _scrollRemX = ax - ix;
      _scrollRemY = ay - iy;
      if (ix == 0 && iy == 0) return const [];
      final sy = settings.natural ? iy : -iy;
      return [OutScroll(ix, sy)];
    }
    if (id != _primary || _dragLockedHeld) return const [];
    _travel += dx.abs() + dy.abs();
    if (_maybeDrag && !_dragging) {
      if (_travel > dragSlopPx) {
        _maybeDrag = false;
        _dragging = true;
        _lastMoveT = t;
        return [const OutDown(buttonLeft), ..._gainMove(dx, dy, t)];
      }
      return const [];
    }
    final out = _gainMove(dx, dy, t);
    return out;
  }

  List<GestureOut> _gainMove(double dx, double dy, int t) {
    final dtMs = (t - _lastMoveT).clamp(1, 1000);
    _lastMoveT = t;
    final dist = (dx.abs() + dy.abs());
    final speed = dist / dtMs * 1000.0;
    final g = pointerGain(speed, settings.sensitivity, settings.accel);
    final x = dx * g + _remX;
    final y = dy * g + _remY;
    final ix = x.truncate();
    final iy = y.truncate();
    _remX = x - ix;
    _remY = y - iy;
    if (ix == 0 && iy == 0) return const [];
    return [OutMove(ix, iy)];
  }

  /// Periodic check for tap-hold (call ~every 50 ms while touching).
  List<GestureOut> tick(int t) {
    if (_maybeDrag && !_dragging && _touches.length == 1) {
      if (t - _downT >= dragHoldMs && _travel <= dragSlopPx) {
        _maybeDrag = false;
        _dragging = true;
        return const [OutDown(buttonLeft)];
      }
    }
    return const [];
  }

  List<GestureOut> pointerUp(int id, double x, double y, int t) {
    final wasTouch = _touches.remove(id);
    if (wasTouch == null) return const [];
    if (_palm) {
      if (_touches.isEmpty) _palm = false;
      return const [];
    }
    if (_threeTap) {
      // Last of three lifts: decide tap (middle click) by the usual
      // quickness + travel test, all reset regardless of outcome.
      if (_touches.isNotEmpty) return const [];
      _threeTap = false;
      final dt = t - _threeDownT;
      final quick = dt <= tapMaxMs && _threeTravel <= tapMaxPx * 2;
      if (quick && settings.tapToClick) {
        return const [OutClick(buttonMiddle)];
      }
      return const [];
    }
    if (_scrolling) {
      if (_touches.length < 2) {
        _scrolling = false;
        final dt = t - _twoDownT;
        final quick = dt <= tapMaxMs && _twoTravel <= tapMaxPx * 2;
        // Rebase a remaining finger so continued one-finger use has no jump.
        if (_touches.length == 1) {
          final rest = _touches.entries.single;
          _primary = rest.key;
          _downT = t;
          _downX = rest.value.x;
          _downY = rest.value.y;
          _travel = 0;
          _lastMoveT = t;
          _remX = 0;
          _remY = 0;
          _maybeDrag = false;
        } else {
          _primary = null;
        }
        if (quick && settings.tapToClick) {
          _lastTapT = -1000000; // two-finger tap is not a drag anchor
          return const [OutClick(buttonRight)];
        }
        return const [];
      }
      return const [];
    }
    if (id != _primary) return const [];
    _primary = null;
    if (_dragging) {
      _dragging = false;
      _maybeDrag = false;
      if (settings.dragLock) {
        _dragLockedHeld = true;
        return const [];
      }
      return const [OutUp(buttonLeft)];
    }
    if (_noTap) {
      _noTap = false;
      _maybeDrag = false;
      return const [];
    }
    if (_maybeDrag) {
      // Fast second press+release = second tap.
      _maybeDrag = false;
      final dt = t - _downT;
      if (dt <= tapMaxMs && _travel <= tapMaxPx && settings.tapToClick) {
        _lastTapT = t;
        _lastTapX = x;
        _lastTapY = y;
        return const [OutClick(buttonLeft)];
      }
      return const [];
    }
    final dt = t - _downT;
    final dx = (x - _downX).abs();
    final dy = (y - _downY).abs();
    if (dt <= tapMaxMs &&
        dx <= tapMaxPx &&
        dy <= tapMaxPx &&
        settings.tapToClick) {
      _lastTapT = t;
      _lastTapX = x;
      _lastTapY = y;
      return const [OutClick(buttonLeft)];
    }
    return const [];
  }

  double _lastTapX = 0;
  double _lastTapY = 0;
}
