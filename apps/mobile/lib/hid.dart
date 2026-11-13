/// USB HID keyboard usage codes (page 0x07) shared by the phone keyboard
/// screen and the desktop's `hid_to_vk` mapping (ADR-0005). `usage` on the
/// wire is one of these; the desktop maps it to the platform code.
abstract final class Hid {
  // Modifier bitmask for KEY events (matches trackpad-core `key` module).
  static const int modShift = 1 << 0;
  static const int modCtrl = 1 << 1;
  static const int modAlt = 1 << 2;
  static const int modMeta = 1 << 3;

  // KEY state byte.
  static const int stateUp = 0;
  static const int stateDown = 1;

  // Whitespace and control.
  static const int enter = 0x28;
  static const int escape = 0x29;
  static const int backspace = 0x2a;
  static const int tab = 0x2b;

  // Editing and navigation.
  static const int insert = 0x49;
  static const int home = 0x4a;
  static const int pageUp = 0x4b;
  static const int delete = 0x4c;
  static const int end = 0x4d;
  static const int pageDown = 0x4e;

  // Arrows.
  static const int arrowRight = 0x4f;
  static const int arrowLeft = 0x50;
  static const int arrowDown = 0x51;
  static const int arrowUp = 0x52;

  // F-keys: usage 0x3A..0x45 = F1..F12.
  static int fKey(int n) => 0x39 + n;
}
