import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate {
  let flutterEngine = FlutterEngine(name: "phone-trackpad")
  private let bonjour = TrackpadBonjour()
  private var discoveryChannel: FlutterMethodChannel?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    flutterEngine.run()
    GeneratedPluginRegistrant.register(with: flutterEngine)
    let channel = FlutterMethodChannel(
        name: "com.kavd.phonetrackpad/bonjour", binaryMessenger: flutterEngine.binaryMessenger)
      channel.setMethodCallHandler { [weak self] call, result in
        guard let self = self else { return }
        guard call.method == "browse" else {
          result(FlutterMethodNotImplemented)
          return
        }
        let args = call.arguments as? [String: Any]
        let milliseconds = (args?["timeoutMs"] as? NSNumber)?.doubleValue ?? 5000
        self.bonjour.browse(timeout: min(max(milliseconds / 1000, 0.1), 30)) { rows in
          result(rows)
        }
      }
    discoveryChannel = channel
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }
  func stopDiscovery() {
    bonjour.finish()
  }
}

// Flutter 3.35 supports scene-aware views, but does not provide the newer
// implicit-engine registration callback. Own one engine explicitly instead.
@objc class SceneDelegate: FlutterSceneDelegate {
  override func scene(
    _ scene: UIScene,
    willConnectTo session: UISceneSession,
    options connectionOptions: UIScene.ConnectionOptions
  ) {
    guard let windowScene = scene as? UIWindowScene,
          let appDelegate = UIApplication.shared.delegate as? AppDelegate else { return }
    let sceneWindow = UIWindow(windowScene: windowScene)
    sceneWindow.rootViewController = FlutterViewController(
      engine: appDelegate.flutterEngine, nibName: nil, bundle: nil)
    window = sceneWindow
    sceneWindow.makeKeyAndVisible()
  }

  override func sceneDidEnterBackground(_ scene: UIScene) {
    // Flutter 3.35's FlutterSceneDelegate does not implement this optional
    // callback. FlutterViewController observes scene notifications itself;
    // forwarding this selector to super crashes at runtime.
    (UIApplication.shared.delegate as? AppDelegate)?.stopDiscovery()
  }

  override func sceneDidDisconnect(_ scene: UIScene) {
    (UIApplication.shared.delegate as? AppDelegate)?.stopDiscovery()
  }
}
