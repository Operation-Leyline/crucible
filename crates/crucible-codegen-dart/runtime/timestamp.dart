import 'json.dart';

/// Exact epoch seconds and a nonnegative nanosecond fraction.
/// DateTime conversions are conveniences with the platform's DateTime precision.
final class Timestamp {
  final BigInt seconds;
  final int nanoseconds;
  static final _billion = BigInt.from(1000000000);
  static final _min = BigInt.parse('-9223372036854775808');
  static final _max = BigInt.parse('9223372036854775807');
  Timestamp(this.seconds, [this.nanoseconds = 0]) {
    if (seconds < _min ||
        seconds > _max ||
        nanoseconds < 0 ||
        nanoseconds >= 1000000000) {
      throw const FormatException('Timestamp outside supported range');
    }
  }
  factory Timestamp.fromDateTime(DateTime value) {
    final millis = value.millisecondsSinceEpoch;
    final remainder = millis % 1000;
    return Timestamp(
      BigInt.from((millis - remainder) ~/ 1000),
      remainder * 1000000 + value.microsecond * 1000,
    );
  }
  DateTime toDateTime() {
    final millis = seconds * BigInt.from(1000);
    final limit = BigInt.parse('8640000000000000');
    if (millis < -limit || millis > limit)
      throw const FormatException('Timestamp outside DateTime range');
    return DateTime.fromMillisecondsSinceEpoch(
      millis.toInt(),
      isUtc: true,
    ).add(Duration(microseconds: nanoseconds ~/ 1000));
  }

  factory Timestamp.fromEpoch(JsonNumber value) {
    final total = value.scaled(9);
    final nanos = total.remainder(_billion);
    final whole = total ~/ _billion;
    return nanos.isNegative
        ? Timestamp(whole - BigInt.one, (nanos + _billion).toInt())
        : Timestamp(whole, nanos.toInt());
  }
  JsonNumber toEpoch() {
    final total = seconds * _billion + BigInt.from(nanoseconds);
    final magnitude = total.abs();
    final whole = magnitude ~/ _billion;
    final fraction = (magnitude % _billion)
        .toString()
        .padLeft(9, '0')
        .replaceFirst(RegExp(r'0+$'), '');
    return JsonNumber(
      '${total.isNegative ? '-' : ''}$whole${fraction.isEmpty ? '' : '.$fraction'}',
    );
  }

  static final _iso = RegExp(
    r'^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(Z|[+-]\d{2}:\d{2})$',
  );
  factory Timestamp.parse(String text) {
    final match = _iso.firstMatch(text);
    if (match == null) throw const FormatException('Invalid ISO timestamp');
    final year = int.parse(match[1]!);
    final month = int.parse(match[2]!);
    final day = int.parse(match[3]!);
    final hour = int.parse(match[4]!);
    final minute = int.parse(match[5]!);
    final second = int.parse(match[6]!);
    final date = _calendar(year, month, day, hour, minute, second);
    final fraction = match[7] ?? '';
    if (fraction.length > 9 &&
        fraction.substring(9).contains(RegExp(r'[1-9]'))) {
      throw const FormatException('Timestamp exceeds nanosecond precision');
    }
    final nanos = int.parse(fraction.padRight(9, '0').substring(0, 9));
    final zone = match[8]!;
    var offset = 0;
    if (zone != 'Z') {
      final hours = int.parse(zone.substring(1, 3));
      final minutes = int.parse(zone.substring(4, 6));
      if (hours > 23 || minutes > 59)
        throw const FormatException('Invalid timestamp offset');
      offset = (hours * 60 + minutes) * 60 * (zone[0] == '-' ? -1 : 1);
    }
    return Timestamp(
      BigInt.from(date.millisecondsSinceEpoch ~/ 1000 - offset),
      nanos,
    );
  }
  static DateTime _calendar(
    int year,
    int month,
    int day,
    int hour,
    int minute,
    int second,
  ) {
    if (year < 1 ||
        year > 9999 ||
        month < 1 ||
        month > 12 ||
        day < 1 ||
        day > 31 ||
        hour > 23 ||
        minute > 59 ||
        second > 59) {
      throw const FormatException('Invalid calendar timestamp');
    }
    final date = DateTime.utc(year, month, day, hour, minute, second);
    if (date.year != year || date.month != month || date.day != day) {
      throw const FormatException('Invalid calendar timestamp');
    }
    return date;
  }

  DateTime _textDate() {
    if (seconds < BigInt.from(-62135596800) ||
        seconds > BigInt.from(253402300799)) {
      throw const FormatException(
        'Text timestamps require years 0001 through 9999',
      );
    }
    return DateTime.fromMillisecondsSinceEpoch(
      (seconds * BigInt.from(1000)).toInt(),
      isUtc: true,
    );
  }

  String toIso8601String() {
    final date = _textDate();
    final base =
        '${date.year.toString().padLeft(4, '0')}-${_two(date.month)}-${_two(date.day)}T${_two(date.hour)}:${_two(date.minute)}:${_two(date.second)}';
    final fraction = nanoseconds
        .toString()
        .padLeft(9, '0')
        .replaceFirst(RegExp(r'0+$'), '');
    return '$base${fraction.isEmpty ? '' : '.$fraction'}Z';
  }

  static const _days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  static const _months = [
    'Jan',
    'Feb',
    'Mar',
    'Apr',
    'May',
    'Jun',
    'Jul',
    'Aug',
    'Sep',
    'Oct',
    'Nov',
    'Dec',
  ];
  static final _http = RegExp(
    r'^(Mon|Tue|Wed|Thu|Fri|Sat|Sun), (\d{2}) (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) (\d{4}) (\d{2}):(\d{2}):(\d{2}) GMT$',
  );
  factory Timestamp.parseHttp(String text) {
    final match = _http.firstMatch(text);
    if (match == null) throw const FormatException('Invalid HTTP timestamp');
    final date = _calendar(
      int.parse(match[4]!),
      _months.indexOf(match[3]!) + 1,
      int.parse(match[2]!),
      int.parse(match[5]!),
      int.parse(match[6]!),
      int.parse(match[7]!),
    );
    if (_days[date.weekday - 1] != match[1])
      throw const FormatException('Incorrect HTTP weekday');
    return Timestamp(BigInt.from(date.millisecondsSinceEpoch ~/ 1000));
  }
  String toHttpDate() {
    final date = _textDate();
    return '${_days[date.weekday - 1]}, ${_two(date.day)} ${_months[date.month - 1]} ${date.year.toString().padLeft(4, '0')} ${_two(date.hour)}:${_two(date.minute)}:${_two(date.second)} GMT';
  }

  static String _two(int value) => value.toString().padLeft(2, '0');
  @override
  bool operator ==(Object other) =>
      other is Timestamp &&
      seconds == other.seconds &&
      nanoseconds == other.nanoseconds;
  @override
  int get hashCode => Object.hash(seconds, nanoseconds);
}
