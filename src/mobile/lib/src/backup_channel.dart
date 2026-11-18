import 'dart:async';

import 'package:flutter/services.dart';

import 'backup_status.dart';

/// A failure of a command or of the status stream, named as in `contracts/17_android_app/status_channel.md`.
/// The screen chooses its text from the type, except [UpgradeRequired], which carries the server's text.
sealed class BackupChannelError implements Exception {
  const BackupChannelError();
}

class HttpsRequired extends BackupChannelError {
  const HttpsRequired();
}

class ServerUnreachable extends BackupChannelError {
  const ServerUnreachable();
}

class InvalidServerUrl extends BackupChannelError {
  const InvalidServerUrl();
}

class UpgradeRequired extends BackupChannelError {
  const UpgradeRequired(this.message);

  final String message;
}

/// A command argument was refused; [field] names it.
class InvalidField extends BackupChannelError {
  const InvalidField(this.field);

  final String field;
}

/// The checkpoint database could not be read, so no status can be sent.
class CheckpointUnavailable extends BackupChannelError {
  const CheckpointUnavailable();
}

/// Native code failed in a way the contract does not name; the cause is in the device log, never in this object.
class UnexpectedNativeError extends BackupChannelError {
  const UnexpectedNativeError(this.code);

  final String code;
}

/// The Dart client of the status channel. Native code owns the backup library; this class only listens and sends the
/// commands of the contract. Native-side channel names are in `ChannelContract.kt`.
class BackupChannel {
  BackupChannel({
    MethodChannel? commands,
    EventChannel? events,
  })  : _commands = commands ?? const MethodChannel('org.sunnycloud.backup/commands'),
        _events = events ?? const EventChannel('org.sunnycloud.backup/status');

  final MethodChannel _commands;
  final EventChannel _events;

  /// A full snapshot when listening starts and after every change. Failures arrive as [BackupChannelError]s on the
  /// stream, a snapshot this version cannot read as a [FormatException].
  Stream<BackupStatus> get status => _events.receiveBroadcastStream().transform(
        StreamTransformer<Object?, BackupStatus>.fromHandlers(
          handleData: (event, sink) {
            if (event is! Map<Object?, Object?>) {
              sink.addError(FormatException('a status event is not a map', event));
              return;
            }
            try {
              sink.add(BackupStatus.fromMap(event));
            } on FormatException catch (e, stack) {
              sink.addError(e, stack);
            }
          },
          handleError: (error, stack, sink) => sink.addError(error is PlatformException ? _typed(error) : error, stack),
        ),
      );

  /// Checks the server address in native code; resolves with how its certificate is trusted.
  Future<ServerTrust> setServer(String url) async {
    final result = await _invoke<Map<Object?, Object?>>('setServer', {'url': url});
    return ServerTrust.fromMap(result);
  }

  /// Stores the settings natively and resolves with what was stored.
  Future<BackupSettings> updateSettings(BackupSettings settings) async {
    final result = await _invoke<Map<Object?, Object?>>('updateSettings', settings.toMap());
    return BackupSettings.fromMap(result);
  }

  Future<T> _invoke<T>(String method, Object? arguments) async {
    try {
      final result = await _commands.invokeMethod<T>(method, arguments);
      return result ?? (throw FormatException('command "$method" returned no result'));
    } on PlatformException catch (e) {
      throw _typed(e);
    }
  }
}

BackupChannelError _typed(PlatformException error) {
  final details = error.details;
  String? detail(String key) {
    final value = details is Map ? details[key] : null;
    return value is String ? value : null;
  }

  final message = detail('message');
  final field = detail('field');
  return switch (error.code) {
    'HttpsRequired' => const HttpsRequired(),
    'ServerUnreachable' => const ServerUnreachable(),
    'InvalidServerUrl' => const InvalidServerUrl(),
    'CheckpointUnavailable' => const CheckpointUnavailable(),
    'UpgradeRequired' when message != null => UpgradeRequired(message),
    'Invalid' when field != null => InvalidField(field),
    _ => UnexpectedNativeError(error.code),
  };
}
