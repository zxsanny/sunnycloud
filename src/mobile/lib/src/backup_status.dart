/// The types of `contracts/17_android_app/status_channel.md`: what the native layer sends and what Dart may send back.
/// None of them has a member for a token, a one-time code or a PKCE verifier, and none may get one.
library;

enum PauseReason {
  quotaExceeded,
  noNetwork,
  meteredNetwork,
  permissionDenied,
  limitedAccess,
  serverRejectedName,
  upgradeRequired,
  accountNotActive,
  signedOut,
}

enum AccessLevel { full, limited, denied }

enum ServerTrust {
  system,
  userAdded;

  /// The `trust` field of a `setServer` result.
  static ServerTrust fromMap(Map<Object?, Object?> map) => _enum(values, map, 'trust');
}

/// Skipped assets by reason; together with `uploaded` and `pending` they add up to `visible`.
class SkippedCounts {
  const SkippedCounts({required this.gone, required this.name, required this.size, required this.failed});

  factory SkippedCounts.fromMap(Map<Object?, Object?> map) => SkippedCounts(
        gone: _int(map, 'gone'),
        name: _int(map, 'name'),
        size: _int(map, 'size'),
        failed: _int(map, 'failed'),
      );

  final int gone;
  final int name;
  final int size;
  final int failed;
}

class LastError {
  const LastError({required this.message, required this.correlationId});

  factory LastError.fromMap(Map<Object?, Object?> map) =>
      LastError(message: _string(map, 'message'), correlationId: _string(map, 'correlation_id'));

  final String message;
  final String correlationId;
}

/// The user settings. [buckets] lists the selected albums; null means every album.
class BackupSettings {
  const BackupSettings({required this.unmeteredOnly, required this.chargingOnly, this.buckets});

  factory BackupSettings.fromMap(Map<Object?, Object?> map) => BackupSettings(
        unmeteredOnly: _bool(map, 'unmetered_only'),
        chargingOnly: _bool(map, 'charging_only'),
        buckets: _optionalStrings(map, 'buckets'),
      );

  final bool unmeteredOnly;
  final bool chargingOnly;
  final List<String>? buckets;

  Map<String, Object?> toMap() => {
        'unmetered_only': unmeteredOnly,
        'charging_only': chargingOnly,
        'buckets': buckets,
      };

  @override
  bool operator ==(Object other) =>
      other is BackupSettings &&
      other.unmeteredOnly == unmeteredOnly &&
      other.chargingOnly == chargingOnly &&
      _sameBuckets(other.buckets, buckets);

  @override
  int get hashCode => Object.hash(unmeteredOnly, chargingOnly, buckets == null ? null : Object.hashAll(buckets!));

  static bool _sameBuckets(List<String>? a, List<String>? b) {
    if (a == null || b == null) return a == b;
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }
}

/// One full snapshot of the backup, as sent on every change and once when Dart subscribes.
class BackupStatus {
  const BackupStatus({
    required this.signedIn,
    required this.serverUrl,
    required this.serverTrust,
    required this.deviceLabel,
    required this.running,
    required this.paused,
    required this.visible,
    required this.uploaded,
    required this.pending,
    required this.skipped,
    required this.lastError,
    required this.accessLevel,
    required this.locationRedacted,
    required this.batteryExempt,
    required this.settings,
  });

  /// Fields the native side sends that this version does not know are ignored (non-breaking additions).
  factory BackupStatus.fromMap(Map<Object?, Object?> map) {
    final lastError = map['last_error'];
    return BackupStatus(
      signedIn: _bool(map, 'signed_in'),
      serverUrl: _optionalString(map, 'server_url'),
      serverTrust: _optionalEnum(ServerTrust.values, map, 'server_trust'),
      deviceLabel: _optionalString(map, 'device_label'),
      running: _bool(map, 'running'),
      paused: _optionalEnum(PauseReason.values, map, 'paused'),
      visible: _int(map, 'visible'),
      uploaded: _int(map, 'uploaded'),
      pending: _int(map, 'pending'),
      skipped: SkippedCounts.fromMap(_map(map, 'skipped')),
      lastError: lastError == null ? null : LastError.fromMap(_map(map, 'last_error')),
      accessLevel: _enum(AccessLevel.values, map, 'access_level'),
      locationRedacted: _bool(map, 'location_redacted'),
      batteryExempt: _bool(map, 'battery_exempt'),
      settings: BackupSettings.fromMap(_map(map, 'settings')),
    );
  }

  final bool signedIn;
  final String? serverUrl;
  final ServerTrust? serverTrust;
  final String? deviceLabel;
  final bool running;
  final PauseReason? paused;
  final int visible;
  final int uploaded;
  final int pending;
  final SkippedCounts skipped;
  final LastError? lastError;
  final AccessLevel accessLevel;
  final bool locationRedacted;
  final bool batteryExempt;
  final BackupSettings settings;
}

/// A field of the wrong shape means native code and this file disagree; it is an error, never a default.
Never _bad(String field, Object? value) => throw FormatException('status field "$field" has an unexpected value', value);

bool _bool(Map<Object?, Object?> map, String field) {
  final value = map[field];
  return value is bool ? value : _bad(field, value);
}

int _int(Map<Object?, Object?> map, String field) {
  final value = map[field];
  return value is int ? value : _bad(field, value);
}

String _string(Map<Object?, Object?> map, String field) {
  final value = map[field];
  return value is String ? value : _bad(field, value);
}

String? _optionalString(Map<Object?, Object?> map, String field) {
  final value = map[field];
  return value == null || value is String ? value as String? : _bad(field, value);
}

Map<Object?, Object?> _map(Map<Object?, Object?> map, String field) {
  final value = map[field];
  return value is Map<Object?, Object?> ? value : _bad(field, value);
}

List<String>? _optionalStrings(Map<Object?, Object?> map, String field) {
  final value = map[field];
  if (value == null) return null;
  return value is List && value.every((e) => e is String) ? List<String>.unmodifiable(value) : _bad(field, value);
}

/// Wire names are `QuotaExceeded` and `user_added`; both match the Dart name once case and `_` are ignored.
T _enum<T extends Enum>(List<T> values, Map<Object?, Object?> map, String field) {
  final wire = _string(map, field).replaceAll('_', '').toLowerCase();
  for (final value in values) {
    if (value.name.toLowerCase() == wire) return value;
  }
  return _bad(field, map[field]);
}

T? _optionalEnum<T extends Enum>(List<T> values, Map<Object?, Object?> map, String field) =>
    map[field] == null ? null : _enum(values, map, field);
