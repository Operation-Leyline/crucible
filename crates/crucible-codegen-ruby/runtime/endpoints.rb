# frozen_string_literal: true
require 'uri'
require 'ipaddr'
module CrucibleGenerated
  module Runtime
    class EndpointError < ::ArgumentError; end
    # Evaluates the endpoint rules IR lowered by crucible-smithy; never Smithy traits.
    module Endpoints
      LABEL = /\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\z/
      ResolvedEndpoint = ::Struct.new(:url, :headers, :properties, keyword_init: true)
      module_function

      def host_label?(value, subdomains = false)
        return false unless value.is_a?(::String) && !value.empty?
        (subdomains ? value.split('.', -1) : [value]).all? { |label| LABEL.match?(label) }
      end

      def parse_url(value)
        return nil unless value.is_a?(::String) && !/[\x00-\x20\x7f?#]/.match?(value)
        uri = ::URI.parse(value)
        return nil unless %w[http https].include?(uri.scheme) && uri.host && !uri.host.empty? && uri.userinfo.nil?
        return nil if uri.port && !uri.port.between?(0, 65_535)
        host = uri.hostname
        ip = begin
          ::IPAddr.new(host)
          true
        rescue ::IPAddr::Error, ::ArgumentError
          false
        end
        return nil unless ip || host_label?(host.delete_suffix('.'), true)
        authority = host.include?(':') ? "[#{host.downcase}]" : host.downcase
        authority += ":#{uri.port}" unless uri.port == uri.default_port
        path = uri.path.empty? ? '/' : uri.path
        {'scheme' => uri.scheme, 'authority' => authority, 'path' => path,
         'normalizedPath' => path.end_with?('/') ? path : path + '/', 'isIp' => ip}
      rescue ::URI::Error
        nil
      end

      def attribute(value, path)
        steps = if path.is_a?(::String)
          path.split('.').flat_map do |part|
            key, index = part.split('[', 2)
            [(key.nil? || key.empty? ? nil : {'kind' => 'member', 'name' => key}),
             (index ? {'kind' => 'index', 'index' => ::Kernel.Integer(index.delete_suffix(']'), 10)} : nil)].compact
          end
        else path
        end
        steps.each do |step|
          if step.fetch('kind') == 'member'
            value = value.is_a?(::Hash) ? value[step.fetch('name')] : nil
          else
            index = step.fetch('index')
            value = value.is_a?(::Array) && index.between?(-value.size, value.size - 1) ? value[index] : nil
          end
          return nil if value.nil?
        end
        value
      end

      def encode(text)
        WireCodec.string(text).b.bytes.map { |byte| (65..90).cover?(byte) || (97..122).cover?(byte) || (48..57).cover?(byte) || [45, 46, 95, 126].include?(byte) ? byte.chr : format('%%%02X', byte) }.join
      end

      def function(name, args)
        case name
        when 'isSet' then !args[0].nil?
        when 'not'
          raise EndpointError, 'Endpoint not expects a boolean' unless args[0] == true || args[0] == false
          !args[0]
        when 'booleanEquals' then args.all? { |value| value == true || value == false } && args[0] == args[1]
        when 'stringEquals' then args.all?(::String) && args[0] == args[1]
        when 'getAttr' then attribute(args[0], args[1])
        when 'substring'
          text, start, stop, reverse = args
          return nil unless text.is_a?(::String) && text.ascii_only? && start.is_a?(::Integer) && stop.is_a?(::Integer) && start >= 0 && start <= stop && stop <= text.size
          reverse ? text[(text.size - stop)...(text.size - start)] : text[start...stop]
        when 'isValidHostLabel' then host_label?(args[0], args[1] == true)
        when 'parseUrl' then parse_url(args[0])
        when 'uriEncode' then encode(args[0])
        else raise EndpointError, "Unsupported endpoint function #{name}"
        end
      end

      def expression(node, values)
        case node.fetch('kind')
        when 'reference' then values[node.fetch('name')]
        when 'boolean', 'integer' then node.fetch('value')
        when 'array' then node.fetch('values').map { |item| expression(item, values) }
        when 'object' then node.fetch('values').transform_values { |item| expression(item, values) }
        when 'call' then function(node.fetch('function'), node.fetch('arguments').map { |item| expression(item, values) })
        when 'string'
          node.fetch('parts').map do |part|
            value = part.fetch('kind') == 'literal' ? part.fetch('value') : attribute(values[part.fetch('name')], part.fetch('path'))
            case value
            when ::String then value
            when true, false, ::Integer then value.to_s
            else raise EndpointError, 'Endpoint template references a missing or non-scalar value'
            end
          end.join
        else raise EndpointError, "Unsupported endpoint expression #{node['kind']}"
        end
      end

      def rules(nodes, parameters)
        nodes.each do |rule|
          scope = parameters.dup
          matched = rule.fetch('conditions').all? do |condition|
            value = function(condition.fetch('function'), condition.fetch('arguments').map { |item| expression(item, scope) })
            next false if value.nil? || value == false
            scope[condition['assign']] = value if condition['assign']
            true
          end
          next unless matched
          result = rule.fetch('result')
          case result.fetch('kind')
          when 'tree'
            nested = rules(result.fetch('rules'), scope)
            return nested if nested
          when 'error' then raise EndpointError, expression(result.fetch('message'), scope)
          else
            url = expression(result.fetch('url'), scope)
            raise EndpointError, 'Endpoint rule returned an invalid HTTP(S) URL' unless parse_url(url)
            headers = result.fetch('headers').to_h do |key, items|
              [key, items.map { |item| expression(item, scope).tap { |text| raise EndpointError, 'Endpoint header values must be strings' unless text.is_a?(::String) } }]
            end
            return ResolvedEndpoint.new(url: url, headers: headers, properties: result.fetch('properties').transform_values { |item| expression(item, scope) })
          end
        end
        nil
      end

      # Parameters come, in order, from each source the IR lists for the operation.
      def resolve(rule_set, sources, input, client, builtins)
        parameters = rule_set.fetch('parameters').to_h do |name, parameter|
          value = nil
          sources.fetch(name, []).each do |source|
            value = case source.fetch('kind')
            when 'static', 'default' then source['value']
            when 'inputMember' then input.is_a?(::Hash) ? input[source.fetch('member')] : nil
            when 'inputPath' then Query.evaluate(source.fetch('path').fetch('expression'), input)
            when 'client' then client[source.fetch('name')]
            when 'builtIn' then builtins[source.fetch('name')]
            end
            break unless value.nil?
          end
          raise EndpointError, "Required endpoint parameter #{name} is missing" if value.nil? && parameter['required']
          unless value.nil?
            valid = case parameter.fetch('kind')
            when 'string' then value.is_a?(::String)
            when 'boolean' then value == true || value == false
            else value.is_a?(::Array) && value.all?(::String)
            end
            raise EndpointError, "Endpoint parameter #{name} has the wrong type" unless valid
          end
          [name, value]
        end
        rules(rule_set.fetch('rules'), parameters) || raise(EndpointError, 'No endpoint rule matched')
      end

      def host_prefix(url, description, input)
        prefix = description.fetch('parts').map do |part|
          next part.fetch('value') if part.fetch('kind') == 'literal'
          value = input.is_a?(::Hash) ? input[part.fetch('member')] : nil
          raise EndpointError, "Invalid endpoint host label #{part['member']}" unless host_label?(value)
          value
        end.join
        uri = ::URI.parse(url)
        host = prefix + uri.host
        raise EndpointError, 'Invalid endpoint hostname after prefix expansion' unless host.size <= 253 && host_label?(host, true)
        uri.host = host
        uri.to_s
      end
    end
  end
end
