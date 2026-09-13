Wavelink — combined iOS app source (Emitter + Receiver in one module, role picker).

Compile gate (single self-contained module — cores inlined, no external module needed):
  cd platform/ios
  xcrun -sdk iphonesimulator swiftc -target arm64-apple-ios14.0-simulator -parse-as-library \
    -typecheck -warnings-as-errors Sources/WavelinkApp/*.swift Sources/WavelinkApp/Shared/*.swift \
    Sources/WavelinkApp/Emitter/*.swift Sources/WavelinkApp/Receiver/*.swift

Result 2026-09-13: 0 errors. Device .ipa + signing remain Apple-credential gates.
The former split (ios-emitter/ios-receiver as separate shells + SPM cores) is superseded
by this merged app for the user layer; the cores are inlined so the app is self-contained.
