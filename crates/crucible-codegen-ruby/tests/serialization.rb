# frozen_string_literal: true
require 'weather_client'
require 'json'
include Weather::Runtime

def exact_tree(value)
  case value
  when JsonNumber then value.rational
  when Integer then value.to_r
  when Array then value.map { |item| exact_tree(item) }
  when Hash then value.transform_values { |item| exact_tree(item) }
  else value
  end
end
codec = Weather.codec
cases = JSON.parse(File.read(File.expand_path('../../../tests/conformance/serialization.json', __dir__)))
cases.each do |row|
  begin
    context = codec.context(row.fetch('target'))
    begin
      native = codec.decode(ExactJson.decode(row.fetch('wire')), context)
    rescue ArgumentError, TypeError
      next if row['reject']
      raise
    end
    raise 'Expected rejection' if row['reject']
    expected = row.fetch('expect')
    actual = ExactJson.decode(ExactJson.encode(codec.encode(native, context)))
    raise 'Exact JSON mismatch' unless exact_tree(actual) == exact_tree(ExactJson.decode(expected.fetch('json')))
    expected.fetch('states', {}).each do |member, state|
      value = native.public_send(member.gsub(/([a-z])([A-Z])/, '\1_\2').downcase)
      actual_state = value.equal?(UNSET) ? 'absent' : (value.nil? ? 'null' : 'value')
      raise "Native presence mismatch: #{member}" unless actual_state == state
    end
    raise 'Blob mismatch' if expected['blobHex'] && native.unpack1('H*') != expected['blobHex']
    if (instant = expected['instant'])
      stamp = native.public_send(instant.fetch('member').gsub(/([a-z])([A-Z])/, '\1_\2').downcase)
      raise 'Native instant mismatch' unless stamp.seconds.to_s == instant.fetch('seconds') && stamp.nanoseconds == instant.fetch('nanoseconds')
    end
  rescue StandardError => error
    raise "#{row.fetch('id')}: #{error.message}"
  end
end
puts "Ruby shared serialization: #{cases.length} cases passed"
