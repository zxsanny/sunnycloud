import 'package:flutter/material.dart';

/// The shared Flutter shell. The platform layer (Kotlin on Android, Swift on iOS) owns the Rust library; Dart only
/// shows what it is told and never loads native code itself (ADR-013). Screens are added under `lib/src/`.
class SunnyCloudApp extends StatelessWidget {
  const SunnyCloudApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'SunnyCloud',
      theme: ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: Colors.amber)),
      home: const Scaffold(body: Center(child: Text('SunnyCloud'))),
    );
  }
}
