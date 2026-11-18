import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sunnycloud_mobile/src/backup_channel.dart';
import 'package:sunnycloud_mobile/src/backup_status.dart';

const _commandsName = 'org.sunnycloud.backup/commands';

final _tokenLike = RegExp('token|code|verifier|secret|password|credential|authori[sz]ation|bearer|refresh|jwt', caseSensitive: false);

/// Every quoted word of the sources that define the channel schema: wire field names, command names, error codes.
List<String> _schemaWords(String source) => RegExp(r"'([A-Za-z_]+)'").allMatches(source).map((m) => m.group(1)!).toList();

List<String> _tokenLikeWords(Iterable<String> words) => words.where(_tokenLike.hasMatch).toList();

Set<String> _keys(Object? value) => switch (value) {
      Map() => {for (final e in value.entries) ...{e.key.toString(), ..._keys(e.value)}},
      Iterable() => {for (final e in value) ..._keys(e)},
      _ => {},
    };

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();

  tearDown(() {
    binding.defaultBinaryMessenger.setMockMethodCallHandler(const MethodChannel(_commandsName), null);
  });

  group('the Dart side of the channel schema', () {
    final sources = [
      'lib/src/backup_status.dart',
      'lib/src/backup_channel.dart',
    ].map((path) => File(path).readAsStringSync());

    test('has no field, command or error name like a token, code or verifier', () {
      // Arrange
      final words = sources.expand(_schemaWords).toList();

      // Act
      final offending = _tokenLikeWords(words);

      // Assert
      expect(words.length, greaterThan(30));
      expect(words, containsAll(['signed_in', 'setServer', 'updateSettings', 'correlation_id']));
      expect(offending, isEmpty);
    });

    test('check fails on a seeded token field', () {
      // Arrange
      final seeded = [...sources.expand(_schemaWords), ..._schemaWords("final x = map['access_token'];")];

      // Act
      final offending = _tokenLikeWords(seeded);

      // Assert
      expect(offending, ['access_token']);
    });
  });

  group('what Dart sends', () {
    test('commands carry only the argument names of the contract', () async {
      // Arrange
      final sent = <Object?>[];
      binding.defaultBinaryMessenger.setMockMethodCallHandler(const MethodChannel(_commandsName), (call) async {
        sent.add(call.arguments);
        return call.method == 'setServer'
            ? {'trust': 'system'}
            : {'unmetered_only': true, 'charging_only': false, 'buckets': null};
      });
      final channel = BackupChannel();

      // Act
      await channel.setServer('https://cloud.example.org');
      await channel.updateSettings(const BackupSettings(unmeteredOnly: true, chargingOnly: false, buckets: ['Camera']));

      // Assert
      final keys = sent.expand(_keys).toSet();
      expect(keys, {'url', 'unmetered_only', 'charging_only', 'buckets'});
      expect(_tokenLikeWords(keys), isEmpty);
    });

    test('settings sent to native hold no key other than the three of the contract', () {
      // Arrange
      const settings = BackupSettings(unmeteredOnly: true, chargingOnly: true, buckets: ['A']);

      // Act
      final keys = settings.toMap().keys;

      // Assert
      expect(keys, {'unmetered_only', 'charging_only', 'buckets'});
    });
  });
}
