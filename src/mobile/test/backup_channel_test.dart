import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sunnycloud_mobile/src/backup_channel.dart';
import 'package:sunnycloud_mobile/src/backup_status.dart';

const _statusName = 'org.sunnycloud.backup/status';
const _commandsName = 'org.sunnycloud.backup/commands';

Map<String, Object?> _event({Map<String, Object?> overrides = const {}}) => {
      'signed_in': true,
      'server_url': 'https://cloud.example.org',
      'server_trust': 'user_added',
      'device_label': 'Pixel',
      'running': true,
      'paused': 'QuotaExceeded',
      'visible': 19,
      'uploaded': 10,
      'pending': 5,
      'skipped': {'gone': 1, 'name': 2, 'size': 1, 'failed': 0},
      'last_error': {'message': 'quota', 'correlation_id': 'c-81'},
      'access_level': 'limited',
      'location_redacted': true,
      'battery_exempt': true,
      'settings': {'unmetered_only': false, 'charging_only': true, 'buckets': ['Camera']},
      ...overrides,
    };

/// Plays the native side of the event channel: `onListen` sends [snapshots] (events or errors) in order.
void _mockStatusStream(TestDefaultBinaryMessengerBinding binding, List<Object> snapshots) {
  const codec = StandardMethodCodec();
  binding.defaultBinaryMessenger.setMockMessageHandler(_statusName, (message) async {
    final call = codec.decodeMethodCall(message);
    if (call.method == 'listen') {
      for (final snapshot in snapshots) {
        final reply = snapshot is PlatformException
            ? codec.encodeErrorEnvelope(code: snapshot.code, message: snapshot.message, details: snapshot.details)
            : codec.encodeSuccessEnvelope(snapshot);
        // An event goes to Dart as a message on the channel, outside the reply of `listen`.
        await binding.defaultBinaryMessenger.handlePlatformMessage(_statusName, reply, (_) {});
      }
    }
    return codec.encodeSuccessEnvelope(null);
  });
}

void main() {
  final binding = TestWidgetsFlutterBinding.ensureInitialized();

  tearDown(() {
    binding.defaultBinaryMessenger.setMockMessageHandler(_statusName, null);
    binding.defaultBinaryMessenger.setMockMethodCallHandler(const MethodChannel(_commandsName), null);
  });

  group('status events', () {
    test('a scripted snapshot is read as sent and the counts add up', () async {
      // Arrange
      _mockStatusStream(binding, [_event()]);

      // Act
      final status = await BackupChannel().status.first;

      // Assert
      expect(status.signedIn, isTrue);
      expect(status.serverUrl, 'https://cloud.example.org');
      expect(status.serverTrust, ServerTrust.userAdded);
      expect(status.deviceLabel, 'Pixel');
      expect(status.running, isTrue);
      expect(status.paused, PauseReason.quotaExceeded);
      expect(status.uploaded, 10);
      expect(status.pending, 5);
      expect(status.skipped.gone, 1);
      expect(status.skipped.name, 2);
      expect(status.skipped.size, 1);
      expect(status.skipped.failed, 0);
      expect(status.lastError?.correlationId, 'c-81');
      expect(status.lastError?.message, 'quota');
      expect(status.accessLevel, AccessLevel.limited);
      expect(status.locationRedacted, isTrue);
      expect(status.batteryExempt, isTrue);
      expect(status.settings, const BackupSettings(unmeteredOnly: false, chargingOnly: true, buckets: ['Camera']));
      final skipped = status.skipped;
      expect(status.visible, status.uploaded + status.pending + skipped.gone + skipped.name + skipped.size + skipped.failed);
    });

    test('optional fields sent as null are null', () async {
      // Arrange
      _mockStatusStream(binding, [
        _event(overrides: {
          'server_url': null,
          'server_trust': null,
          'device_label': null,
          'paused': null,
          'last_error': null,
          'settings': {'unmetered_only': true, 'charging_only': false, 'buckets': null},
        }),
      ]);

      // Act
      final status = await BackupChannel().status.first;

      // Assert
      expect(status.serverUrl, isNull);
      expect(status.serverTrust, isNull);
      expect(status.deviceLabel, isNull);
      expect(status.paused, isNull);
      expect(status.lastError, isNull);
      expect(status.settings.buckets, isNull);
    });

    test('every pause reason, access level and trust name of the contract is understood', () async {
      // Arrange
      const reasons = [
        'QuotaExceeded', 'NoNetwork', 'MeteredNetwork', 'PermissionDenied', 'LimitedAccess',
        'ServerRejectedName', 'UpgradeRequired', 'AccountNotActive', 'SignedOut',
      ];
      _mockStatusStream(binding, [
        for (final reason in reasons) _event(overrides: {'paused': reason}),
        for (final level in ['full', 'limited', 'denied']) _event(overrides: {'access_level': level}),
        for (final trust in ['system', 'user_added']) _event(overrides: {'server_trust': trust}),
      ]);

      // Act
      final all = await BackupChannel().status.take(reasons.length + 5).toList();

      // Assert
      expect(all.take(reasons.length).map((s) => s.paused), PauseReason.values);
      expect(all.skip(reasons.length).take(3).map((s) => s.accessLevel), AccessLevel.values);
      expect(all.skip(reasons.length + 3).map((s) => s.serverTrust), ServerTrust.values);
    });

    test('fields this version does not know are ignored', () async {
      // Arrange
      _mockStatusStream(binding, [_event(overrides: {'a_future_field': 1})]);

      // Act
      final status = await BackupChannel().status.first;

      // Assert
      expect(status.uploaded, 10);
    });

    test('a snapshot with a wrong field type is a FormatException on the stream', () async {
      // Arrange
      _mockStatusStream(binding, [_event(overrides: {'uploaded': 'ten'})]);

      // Act
      final next = BackupChannel().status.first;

      // Assert
      await expectLater(next, throwsA(isA<FormatException>()));
    });

    test('an unknown pause reason is a FormatException on the stream', () async {
      // Arrange
      _mockStatusStream(binding, [_event(overrides: {'paused': 'Mystery'})]);

      // Act
      final next = BackupChannel().status.first;

      // Assert
      await expectLater(next, throwsA(isA<FormatException>()));
    });

    test('a native error event arrives as a typed error', () async {
      // Arrange
      _mockStatusStream(binding, [PlatformException(code: 'CheckpointUnavailable')]);

      // Act
      final next = BackupChannel().status.first;

      // Assert
      await expectLater(next, throwsA(isA<CheckpointUnavailable>()));
    });
  });

  group('commands', () {
    void mockCommands(Future<Object?> Function(MethodCall call) handler) {
      binding.defaultBinaryMessenger.setMockMethodCallHandler(const MethodChannel(_commandsName), handler);
    }

    test('setServer sends the url and returns the trust', () async {
      // Arrange
      MethodCall? seen;
      mockCommands((call) async {
        seen = call;
        return {'trust': 'user_added'};
      });

      // Act
      final trust = await BackupChannel().setServer('https://cloud.example.org');

      // Assert
      expect(trust, ServerTrust.userAdded);
      expect(seen?.method, 'setServer');
      expect(seen?.arguments, {'url': 'https://cloud.example.org'});
    });

    test('native error codes become the typed errors of the contract', () async {
      // Arrange
      final codes = <String, Matcher>{
        'HttpsRequired': isA<HttpsRequired>(),
        'ServerUnreachable': isA<ServerUnreachable>(),
        'InvalidServerUrl': isA<InvalidServerUrl>(),
        'SomethingNew': isA<UnexpectedNativeError>(),
      };

      for (final entry in codes.entries) {
        mockCommands((call) async => throw PlatformException(code: entry.key));

        // Act
        final result = BackupChannel().setServer('http://cloud.example.org');

        // Assert
        await expectLater(result, throwsA(entry.value));
      }
    });

    test('UpgradeRequired carries the server text', () async {
      // Arrange
      mockCommands((call) async => throw PlatformException(
            code: 'UpgradeRequired',
            message: 'Update the app',
            details: {'message': 'Update the app'},
          ));

      // Act
      final result = BackupChannel().setServer('https://cloud.example.org');

      // Assert
      await expectLater(result, throwsA(isA<UpgradeRequired>().having((e) => e.message, 'message', 'Update the app')));
    });

    test('an Invalid error names the field', () async {
      // Arrange
      mockCommands((call) async => throw PlatformException(code: 'Invalid', details: {'field': 'buckets'}));

      // Act
      final result = BackupChannel().updateSettings(const BackupSettings(unmeteredOnly: true, chargingOnly: false));

      // Assert
      await expectLater(result, throwsA(isA<InvalidField>().having((e) => e.field, 'field', 'buckets')));
    });

    test('an UpgradeRequired error without text is not guessed at', () async {
      // Arrange
      mockCommands((call) async => throw PlatformException(code: 'UpgradeRequired'));

      // Act
      final result = BackupChannel().setServer('https://cloud.example.org');

      // Assert
      await expectLater(result, throwsA(isA<UnexpectedNativeError>()));
    });

    test('updateSettings sends the settings and returns what native stored', () async {
      // Arrange
      MethodCall? seen;
      mockCommands((call) async {
        seen = call;
        return {'unmetered_only': false, 'charging_only': true, 'buckets': <String>['Camera']};
      });
      const settings = BackupSettings(unmeteredOnly: false, chargingOnly: true, buckets: ['Camera', 'Screenshots']);

      // Act
      final stored = await BackupChannel().updateSettings(settings);

      // Assert
      expect(seen?.method, 'updateSettings');
      expect(seen?.arguments, {'unmetered_only': false, 'charging_only': true, 'buckets': ['Camera', 'Screenshots']});
      expect(stored, const BackupSettings(unmeteredOnly: false, chargingOnly: true, buckets: ['Camera']));
    });

    test('a command that native code has not implemented is a MissingPluginException', () async {
      // Arrange
      mockCommands((call) async => throw MissingPluginException());

      // Act
      final result = BackupChannel().setServer('https://cloud.example.org');

      // Assert
      await expectLater(result, throwsA(isA<MissingPluginException>()));
    });

    test('a result of the wrong shape is a FormatException', () async {
      // Arrange
      mockCommands((call) async => {'trust': 'full'});

      // Act
      final result = BackupChannel().setServer('https://cloud.example.org');

      // Assert
      await expectLater(result, throwsA(isA<FormatException>()));
    });
  });
}
