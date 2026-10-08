import 'dart:typed_data';

/// What a string representation prints in place of a `@sensitive` value.
const String redactedMarker = '*** Sensitive Data Redacted ***';

/// Replaces a present sensitive [value] with [redactedMarker]; an absent one
/// stays absent so it is omitted like any other unset member.
Object? redact(Object? value) => value == null ? null : redactedMarker;

/// Renders `TypeName(member=value, ...)` for generated models.
///
/// Members whose value is `null` are omitted, so an unset optional member and
/// one set to an explicit null look alike. Nested models recurse through their
/// own `toString`, collections render their elements, and blobs show only
/// their length so a payload never floods a log line.
///
/// Modeled errors pass their transport [metadata], shown last.
String describe(String type, Map<String, Object?> members, {Object? metadata}) {
  final parts = [
    for (final entry in members.entries)
      if (entry.value != null) '${entry.key}=${describeValue(entry.value)}',
    if (metadata != null) 'metadata=${describeValue(metadata)}',
  ];
  return '$type(${parts.join(', ')})';
}

/// Renders one value the way [describe] renders a member.
String describeValue(Object? value) => switch (value) {
      null => 'null',
      Uint8List(:final length) => '<$length bytes>',
      DateTime() => value.toUtc().toIso8601String(),
      List() => '[${value.map(describeValue).join(', ')}]',
      Map() => '{${value.entries.map((entry) => '${describeValue(entry.key)}=${describeValue(entry.value)}').join(', ')}}',
      _ => '$value',
    };
