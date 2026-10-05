# frozen_string_literal: true
require 'json'
module CrucibleGenerated
  module Runtime
    class QueryError < ::ArgumentError; end
    # Evaluates the portable JMESPath tree compiled by crucible-smithy. Values are
    # JSON-shaped: Hash with String keys, Array, String, true/false/nil, and exact
    # Rational numbers, so decimal comparisons never round through Float.
    module Query
      FUNCTIONS = {
        'abs' => 1, 'avg' => 1, 'ceil' => 1, 'contains' => 2, 'ends_with' => 2, 'floor' => 1, 'join' => 2,
        'keys' => 1, 'length' => 1, 'map' => 2, 'max' => 1, 'max_by' => 2, 'merge' => nil, 'min' => 1,
        'min_by' => 2, 'not_null' => nil, 'reverse' => 1, 'sort' => 1, 'sort_by' => 2, 'starts_with' => 2,
        'sum' => 1, 'to_array' => 1, 'to_number' => 1, 'to_string' => 1, 'type' => 1, 'values' => 1
      }.freeze
      module_function

      def search(expression, value) = evaluate(expression, normalize(value))

      # Converts native JSON-shaped values into the query domain.
      def normalize(value)
        case value
        when JsonNumber then value.rational
        when ::Integer then ::Kernel.Rational(value)
        when ::Float
          raise QueryError, 'Non-finite number in query' unless value.finite?
          value.to_r
        when ::BigDecimal then value.to_r
        when ::Rational then value
        when ::Array then value.map { |item| normalize(item) }
        when ::Hash then value.to_h { |key, item| [key, normalize(item)] }
        when ::String, true, false, nil then value
        when JsonValue then normalize(value.value)
        else raise QueryError, 'Unsupported query value'
        end
      end

      def number?(value) = value.is_a?(::Rational)
      def truthy?(value)
        case value
        when nil, false then false
        when ::String, ::Array, ::Hash then !value.empty?
        else true
        end
      end
      def same?(left, right)
        return number?(right) && left == right if number?(left)
        return false if number?(right)
        if left.is_a?(::Array) && right.is_a?(::Array)
          return left.size == right.size && left.zip(right).all? { |a, b| same?(a, b) }
        end
        if left.is_a?(::Hash) && right.is_a?(::Hash)
          return left.size == right.size && left.all? { |key, item| right.key?(key) && same?(item, right[key]) }
        end
        boolean = ->(value) { value == true || value == false }
        return left == right if boolean.(left) && boolean.(right)
        left.instance_of?(right.class) && left == right
      end
      def text(value)
        raise QueryError, 'JMESPath expected string' unless value.is_a?(::String)
        value
      end
      def array(value)
        raise QueryError, 'JMESPath expected array' unless value.is_a?(::Array)
        value
      end
      def object(value)
        raise QueryError, 'JMESPath expected object' unless value.is_a?(::Hash)
        value
      end
      def numeric(value)
        raise QueryError, 'JMESPath expected number' unless number?(value)
        value
      end
      def integer(value)
        value = value.is_a?(::Integer) ? value : numeric(value)
        raise QueryError, 'JMESPath expected integer' unless value.to_r.denominator == 1
        value.to_i
      end

      def evaluate(node, input)
        node = object(node)
        case node.fetch('kind')
        when 'identity' then input
        when 'literal' then normalize(node['value'])
        when 'field' then input.is_a?(::Hash) ? input[node.fetch('name')] : nil
        when 'index'
          return nil unless input.is_a?(::Array)
          index = integer(node.fetch('index'))
          index += input.size if index.negative?
          index.between?(0, input.size - 1) ? input[index] : nil
        when 'subexpression' then evaluate(node.fetch('right'), evaluate(node.fetch('left'), input))
        when 'projection'
          left = evaluate(node.fetch('left'), input)
          return nil unless left.is_a?(::Array)
          left.filter_map { |item| evaluate(node.fetch('right'), item) }
        when 'flatten'
          value = evaluate(node.fetch('expression'), input)
          return nil unless value.is_a?(::Array)
          value.flat_map { |item| item.is_a?(::Array) ? item : [item] }
        when 'objectValues'
          value = evaluate(node.fetch('expression'), input)
          value.is_a?(::Hash) ? value.values : nil
        when 'multiList'
          input.nil? ? nil : node.fetch('expressions').map { |item| evaluate(item, input) }
        when 'multiHash'
          input.nil? ? nil : node.fetch('entries').to_h { |entry| [entry.fetch('key'), evaluate(entry.fetch('value'), input)] }
        when 'and'
          left = evaluate(node.fetch('left'), input)
          truthy?(left) ? evaluate(node.fetch('right'), input) : left
        when 'or'
          left = evaluate(node.fetch('left'), input)
          truthy?(left) ? left : evaluate(node.fetch('right'), input)
        when 'not' then !truthy?(evaluate(node.fetch('expression'), input))
        when 'condition' then truthy?(evaluate(node.fetch('predicate'), input)) ? evaluate(node.fetch('then'), input) : nil
        when 'compare' then compare(node, input)
        when 'slice' then slice(node, input)
        when 'function' then function(node.fetch('function'), node.fetch('arguments'), input)
        else raise QueryError, 'Unknown compiled JMESPath node'
        end
      end

      def compare(node, input)
        left, right = evaluate(node.fetch('left'), input), evaluate(node.fetch('right'), input)
        case node.fetch('comparator')
        when 'equal' then same?(left, right)
        when 'notEqual' then !same?(left, right)
        else
          return nil unless number?(left) && number?(right)
          case node.fetch('comparator')
          when 'lessThan' then left < right
          when 'lessThanEqual' then left <= right
          when 'greaterThan' then left > right
          when 'greaterThanEqual' then left >= right
          else raise QueryError, 'Unknown JMESPath comparator'
          end
        end
      end

      def slice(node, input)
        return nil unless input.is_a?(::Array)
        step = integer(node.fetch('step'))
        raise QueryError, 'Zero slice step' if step.zero?
        size = input.size
        bound = lambda do |value, fallback|
          return fallback if value.nil?
          value = integer(value)
          value += size if value.negative?
          step.positive? ? value.clamp(0, size) : value.clamp(-1, size - 1)
        end
        start = bound.call(node['start'], step.positive? ? 0 : size - 1)
        stop = bound.call(node['stop'], step.positive? ? size : -1)
        result = []
        index = start
        while step.positive? ? index < stop : index > stop
          result << input[index]
          index += step
        end
        result
      end

      def ordered(values)
        values = array(values)
        return [] if values.empty?
        numeric = number?(values.first)
        values.each { |item| numeric ? numeric(item) : text(item) }
        values.each_with_index.sort_by { |item, index| [item, index] }.map(&:first)
      end
      def keyed(values, keys)
        unless keys.empty?
          numeric = number?(keys.first)
          keys.each { |item| numeric ? numeric(item) : text(item) }
        end
        values.each_index.sort_by { |index| [keys[index], index] }
      end
      def reference(node, input)
        node = object(node)
        raise QueryError, 'JMESPath expected expression reference' unless node.fetch('kind') == 'expressionReference'
        evaluate(node.fetch('expression'), input)
      end
      def json_text(value)
        case value
        when ::Rational
          return value.to_i.to_s if value.denominator == 1
          decimal = ::Kernel.BigDecimal(value, 40)
          raise QueryError, 'Number has no exact decimal text' unless decimal.to_r == value
          decimal.to_s('F')
        when ::Array then '[' + value.map { |item| json_text(item) }.join(',') + ']'
        when ::Hash then '{' + value.map { |key, item| ::JSON.generate(key) + ':' + json_text(item) }.join(',') + '}'
        else ::JSON.generate(value)
        end
      end

      def function(name, nodes, input)
        raise QueryError, 'Unknown compiled JMESPath function' unless FUNCTIONS.key?(name)
        arity = FUNCTIONS[name]
        raise QueryError, 'JMESPath invalid function arity' if arity ? nodes.size != arity : nodes.empty?
        arg = ->(index) { evaluate(nodes[index], input) }
        case name
        when 'abs' then numeric(arg.(0)).abs
        when 'ceil' then ::Kernel.Rational(numeric(arg.(0)).ceil)
        when 'floor' then ::Kernel.Rational(numeric(arg.(0)).floor)
        when 'sum' then array(arg.(0)).sum(::Kernel.Rational(0)) { |item| numeric(item) }
        when 'avg'
          values = array(arg.(0))
          values.empty? ? nil : values.sum(::Kernel.Rational(0)) { |item| numeric(item) } / values.size
        when 'contains'
          source, wanted = arg.(0), arg.(1)
          source.is_a?(::String) ? source.include?(text(wanted)) : array(source).any? { |item| same?(item, wanted) }
        when 'starts_with' then text(arg.(0)).start_with?(text(arg.(1)))
        when 'ends_with' then text(arg.(0)).end_with?(text(arg.(1)))
        when 'join'
          separator = text(arg.(0))
          array(arg.(1)).map { |item| text(item) }.join(separator)
        when 'keys' then object(arg.(0)).keys
        when 'values' then object(arg.(0)).values
        when 'length'
          value = arg.(0)
          ::Kernel.Rational(value.is_a?(::Array) || value.is_a?(::Hash) ? value.size : text(value).length)
        when 'map' then array(arg.(1)).map { |item| reference(nodes[0], item) }
        when 'sort' then ordered(arg.(0))
        when 'max' then ordered(arg.(0)).last
        when 'min' then ordered(arg.(0)).first
        when 'sort_by', 'max_by', 'min_by'
          values = array(arg.(0))
          order = keyed(values, values.map { |item| reference(nodes[1], item) })
          if name == 'sort_by' then order.map { |index| values[index] }
          elsif values.empty? then nil
          else values[name == 'max_by' ? order.last : order.first]
          end
        when 'merge' then nodes.each_index.reduce({}) { |result, index| result.merge(object(arg.(index))) }
        when 'not_null'
          nodes.each_index { |index| value = arg.(index); return value unless value.nil? }
          nil
        when 'reverse'
          value = arg.(0)
          value.is_a?(::Array) ? value.reverse : text(value).reverse
        when 'to_array'
          value = arg.(0)
          value.is_a?(::Array) ? value : [value]
        when 'to_number'
          value = arg.(0)
          if number?(value) then value
          elsif value.is_a?(::String)
            begin
              JsonNumber.new(value).rational
            rescue ::ArgumentError
              nil
            end
          end
        when 'to_string'
          value = arg.(0)
          value.is_a?(::String) ? value : json_text(value)
        when 'type'
          case arg.(0)
          when nil then 'null'
          when true, false then 'boolean'
          when ::String then 'string'
          when ::Rational then 'number'
          when ::Array then 'array'
          else 'object'
          end
        end
      end
    end
  end
end
