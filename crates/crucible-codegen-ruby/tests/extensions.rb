# frozen_string_literal: true
require 'types_client'

codec = Types.codec
node = Types::Runtime::ExactJson.decode('{"mode":123,"names":["a","b"],"sparseNames":[null,"x"],"sparseMap":{"absent":null,"future":123}}')
output = codec.decode(node, codec.context('example.types#ReadOutput'))
raise 'integer' unless output.integer == 12345678901234567890123456789012345678901234567890
raise 'decimal' unless output.decimal == BigDecimal('12345678901234567890.12345678901234567890123456789')
raise 'sparse' unless output.sparse_names == [nil, 'x'] && output.sparse_map.key?('absent') && output.sparse_map['absent'].nil?
raise 'future enum' unless output.mode.value == 123 && output.sparse_map['future'].value == 123
encoded = codec.encode(output, codec.context('example.types#ReadOutput'))
raise 'roundtrip' unless codec.equivalent(encoded['sparseMap'], node['sparseMap']) && encoded['sparseNames'] == node['sparseNames']
[['example.types#Names', ['a', 'a']], ['example.types#Names', [nil]], ['example.types#Mode', 2147483648]].each do |target, value|
  begin
    codec.decode(value, codec.context(target))
    raise 'accepted invalid value'
  rescue ArgumentError
  end
end
puts 'Ruby extended types passed'
{'Integers' => '[18446744073709551616,18446744073709551617]', 'Decimals' => '[1.00000000000000000001,1.00000000000000000002]'}.each do |shape, json|
  codec.decode(Types::Runtime::ExactJson.decode(json), codec.context('example.types#' + shape))
  begin
    codec.decode(Types::Runtime::ExactJson.decode('[1,1.0]'), codec.context('example.types#' + shape))
    raise 'accepted numerically equal duplicate'
  rescue ArgumentError
  end
end
