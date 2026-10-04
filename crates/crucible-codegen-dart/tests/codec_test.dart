import 'dart:typed_data';

import '../runtime/codec.dart';
import '../runtime/json.dart';
import '../runtime/timestamp.dart';
import 'json_test.dart' show check, rejects;

void main() {
  final schema = wireObject(
    exactJsonDecode('''{
    "shapes": {
      "str": {"kind":"string"}, "blob": {"kind":"blob"},
      "long": {"kind":"long"}, "byte": {"kind":"byte"},
      "float": {"kind":"float"}, "double": {"kind":"double"},
      "bool": {"kind":"boolean"}, "doc": {"kind":"document"},
      "time": {"kind":"timestamp", "format":"epoch-seconds"},
      "list": {"kind":"list", "member":{"target":"str"}},
      "structure": {"kind":"structure", "members": {
        "missing":{"target":"str", "default":null},
        "defaulted":{"target":"long", "default":9223372036854775807},
        "required":{"target":"byte", "required":true}
      }}
    }
  }'''),
  );
  final codec = WireCodec(
    schema,
    (target, value, context) =>
        throw StateError('Unexpected generated model $target'),
  );
  final max = BigInt.parse('9223372036854775807');
  check(
    codec.decode(JsonNumber(max.toString()), codec.context('long')) == max,
    'Exact long decode',
  );
  check(
    exactJsonEncode(codec.encode(max, codec.context('long'))) == max.toString(),
    'Exact long encode',
  );
  rejects(
    () => codec.encode(
      BigInt.parse('9223372036854775808'),
      codec.context('long'),
    ),
  );
  rejects(() => codec.encode(128, codec.context('byte')));
  rejects(() => codec.decode(JsonNumber('1.5'), codec.context('byte')));
  rejects(() => codec.decode('true', codec.context('bool')));
  rejects(() => codec.decode(JsonNumber('1e1000'), codec.context('double')));
  rejects(() => codec.decode(JsonNumber('3.5e38'), codec.context('float')));
  check(
    codec.decode(JsonNumber('16777217'), codec.context('float')) == 16777216.0,
    'IEEE float rounding',
  );
  rejects(() => codec.encode(double.nan, codec.context('double')));
  final bytes = Uint8List.fromList([0, 128, 255]);
  final blob = codec.encode(bytes, codec.context('blob'));
  check(blob == 'AID/', 'Base64 bytes');
  check(
    (codec.decode(blob, codec.context('blob')) as Uint8List)[1] == 128,
    'Base64 decode',
  );
  rejects(() => codec.decode('_w==', codec.context('blob')));
  rejects(() => codec.decode('AA', codec.context('blob')));
  final structure = codec.context('structure');
  check(!structure.optional<String>('missing', {}).isSet, 'Omitted member');
  check(
    structure.optional<String>('missing', {'missing': null}).isSet,
    'Explicit null',
  );
  check(
    structure.optional<BigInt>('defaulted', {}).value == max,
    'Exact modeled default',
  );
  rejects(() => structure.read<int>('required', {}));
  final output = <String, Object?>{};
  structure.put(output, 'missing', const Field<String>.unset());
  check(output.isEmpty, 'Omitted encoding');
  structure.put(output, 'missing', const Field<String>.nullValue());
  check(
    output.containsKey('missing') && output['missing'] == null,
    'Null encoding',
  );
  structure.put(output, 'missing', const Field<String>.value('supplied'));
  check(output['missing'] == 'supplied', 'Supplied encoding');
  check(
    (codec.decode(null, codec.context('doc')) as JsonValue).value == null,
    'Document null',
  );
  final timestamp = codec.decode(
    JsonNumber('-0.000000001'),
    codec.context('time'),
  ) as Timestamp;
  check(
    exactJsonEncode(codec.encode(timestamp, codec.context('time'))) ==
        '-0.000000001',
    'Member timestamp format',
  );
  final values = ['a,b', '', ' spaced ', 'quote"', r'back\slash', 'é'];
  final header = codec.bound(values);
  final decoded = codec.header({'target': 'list'}, header) as List;
  check(
    exactJsonEncode(decoded) == exactJsonEncode(values),
    'Quoted header lists',
  );
  check(
    (codec.header({'target': 'list'}, '') as List).isEmpty,
    'Empty header list',
  );
  rejects(() => codec.header({'target': 'list'}, '"unterminated'));
  print('Schema codec qualification passed');
}
