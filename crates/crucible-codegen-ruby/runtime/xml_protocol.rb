# frozen_string_literal: true
require 'rexml/document'
module CrucibleGenerated
  module Runtime
    # The AWS XML family: restXml documents, awsQuery and ec2Query forms, and the XML
    # responses and error envelopes of all three. Values are the codec's wire values, keyed
    # by member name; element and query names come from the lowered xmlShapes.
    class XmlProtocol
      PROTOCOLS = %w[awsRestXml awsQuery ec2Query].freeze
      MAX_DEPTH = 256
      NON_FINITE = %w[NaN Infinity -Infinity].freeze
      # A parsed element; names are qualified as written, so prefix:local or local.
      Node = Struct.new(:name, :local, :attributes, :children, :text) do
        def is?(expected) = name == expected || (!expected.include?(':') && local == expected)
        def child(expected) = children.find { |node| node.is?(expected) }
        def all(expected) = children.select { |node| node.is?(expected) }
        def attribute(expected)
          return attributes[expected] if attributes.key?(expected) || expected.include?(':')
          attributes.find { |key, _| key.split(':').last == expected }&.last
        end
        def find(expected)
          return self if is?(expected)
          children.each do |node|
            found = node.find(expected)
            return found if found
          end
          nil
        end
      end
      ErrorEnvelope = Struct.new(:code, :message, :request_id, :error)

      def self.handles?(protocol) = PROTOCOLS.include?(protocol)

      def initialize(schema)
        @shapes = schema.fetch('shapes')
        @xml_shapes = schema.fetch('xmlShapes', {})
        @service_namespace = schema['xmlNamespace']
        @operations = schema.fetch('operations', {})
        @protocol = schema.fetch('protocol')
        @version = schema.fetch('version')
      end
      def query? = @protocol != 'awsRestXml'

      # A restXml body: the input's body members under its root element.
      def document(target, members, values)
        xml = xml_shape(target)
        out = +'<' << xml.fetch('name')
        declare(out, xml['namespace'] || @service_namespace)
        attributes(out, target, values, members)
        out << '>'
        members.each { |member| child(out, target, member, values[member]) }
        (out << '</' << xml.fetch('name') << '>').encode(::Encoding::UTF_8)
      end

      # A restXml structure or union payload, named by the member's resolved payload name.
      def payload(owner, name, value)
        xml = xml_member(owner, name)
        out = +''
        element(out, xml.fetch('payloadName'), xml['namespace'] || @service_namespace, member(owner, name).fetch('target'), value)
        out
      end

      # An awsQuery or ec2Query request body.
      def form(operation, target, values)
        pairs = ['Action=' + escape(operation), 'Version=' + escape(@version)]
        members(pairs, '', target, values) if target && shape(target).fetch('kind') == 'structure'
        pairs.join('&')
      end

      # The output members of a successful response; only limits them to the body.
      def output(operation, target, only, payload, body)
        return {} if target.nil?
        if payload
          return {} if body.empty?
          return {payload => read(parse(body), member(target, payload).fetch('target'), nil)}
        end
        return {} if only.empty? || body.strip.empty?
        root = parse(body)
        if @operations.fetch(operation, {})['unwrappedXmlOutput']
          name = only.first
          raise ::ArgumentError, 'Unwrapped XML response root differs from its body member' unless root.is?(xml_member(target, name).fetch('name'))
          description = member(target, name)
          return {name => read(root, description.fetch('target'), description['format'])}
        end
        if @protocol == 'awsQuery'
          # awsQuery wraps the members in <OperationResult>, beside <ResponseMetadata>.
          root = root.child(operation + 'Result')
          return {} if root.nil?
        end
        read_members(root, target, only)
      end

      # Finds the error in any of the family's envelopes: <ErrorResponse><Error>, restXml's
      # unwrapped <Error>, or ec2Query's <Response><Errors><Error>.
      def self.error(body)
        root = parse(body)
        error = root.find('Error')
        id = root.find('RequestId') || root.find('RequestID')
        ErrorEnvelope.new(error && text_of(error.child('Code')), error && text_of(error.child('Message')), text_of(id), error)
      rescue ::ArgumentError
        ErrorEnvelope.new(nil, nil, nil, nil)
      end
      def self.text_of(node) = node&.text&.strip

      # Parses untrusted XML: no document type declarations, so no entity can be declared.
      def self.parse(body)
        text = body.dup.force_encoding(::Encoding::UTF_8)
        raise ::ArgumentError, 'XML response is not UTF-8' unless text.valid_encoding?
        raise ::ArgumentError, 'XML document type declarations and entities are not accepted' if text.include?('<!DOCTYPE') || text.include?('<!ENTITY')
        document = ::REXML::Document.new(text)
        root = document.root
        raise ::ArgumentError, 'XML document has no root element' if root.nil?
        convert(root, 0)
      rescue ::REXML::ParseException, ::REXML::UndefinedNamespaceException, ::RuntimeError => error
        raise ::ArgumentError, 'Malformed XML: ' + error.message.lines.first.to_s.strip
      end
      def self.convert(element, depth)
        raise ::ArgumentError, 'XML nesting limit' if depth >= MAX_DEPTH
        attributes = {}
        element.attributes.each_attribute do |attribute|
          next if attribute.prefix == 'xmlns' || attribute.expanded_name == 'xmlns'
          undeclared(attribute.to_s)
          attributes[attribute.expanded_name] = attribute.value
        end
        children, text = [], +''
        element.children.each do |node|
          case node
          when ::REXML::Element then children << convert(node, depth + 1)
          when ::REXML::CData then text << node.value
          when ::REXML::Text
            undeclared(node.to_s)
            text << node.value
          end
        end
        Node.new(element.expanded_name, element.name, attributes, children, text)
      end
      # REXML keeps an undeclared entity reference as literal text; XML forbids it.
      def self.undeclared(raw)
        raise ::ArgumentError, 'XML refers to an undeclared entity' if /&(?!(?:amp|lt|gt|quot|apos|#[0-9]+|#x[0-9A-Fa-f]+);)/.match?(raw)
      end
      def parse(body) = self.class.parse(body)

      def read_members(node, target, only)
        result = {}
        only.each do |name|
          xml, description = xml_member(target, name), member(target, name)
          member_target, format, element = description.fetch('target'), description['format'], xml.fetch('name')
          if xml['attribute']
            value = node.attribute(element)
            result[name] = scalar(member_target, value, format) unless value.nil?
          elsif xml['flattened']
            found = node.all(element)
            result[name] = collection(found, member_target) unless found.empty?
          elsif (child = node.child(element))
            result[name] = read(child, member_target, format)
          end
        end
        result
      end

      private

      def shape(target) = @shapes.fetch(target)
      def xml_shape(target) = @xml_shapes.fetch(target) { raise ::ArgumentError, 'No XML description for ' + target }
      def xml_member(owner, name) = xml_shape(owner).fetch('members').fetch(name)
      # A structure member, or a list or map's member, key or value.
      def member(owner, name)
        info = shape(owner)
        info.key?('members') ? info.fetch('members').fetch(name) : info.fetch(name)
      end

      def declare(out, namespace)
        return unless namespace
        out << (namespace['prefix'] ? ' xmlns:' + namespace['prefix'] : ' xmlns') << '="' << escape_xml(namespace.fetch('uri'), true) << '"'
      end
      def attributes(out, target, value, members)
        members.each do |name|
          xml = xml_member(target, name)
          item = value[name]
          out << ' ' << xml.fetch('name') << '="' << escape_xml(text(item), true) << '"' if xml['attribute'] && !item.nil?
        end
      end
      def element(out, name, namespace, target, value)
        info = shape(target)
        out << '<' << name
        declare(out, namespace)
        case info.fetch('kind')
        when 'structure', 'union'
          object = WireCodec.object(value)
          members = info.fetch('members').keys
          attributes(out, target, object, members)
          out << '>'
          members.each { |member| child(out, target, member, object[member]) }
        when 'list'
          out << '>'
          item = xml_member(target, 'member')
          WireCodec.items(value).each { |entry| element(out, item.fetch('name'), item['namespace'], info.fetch('member').fetch('target'), entry) }
        when 'map'
          out << '>'
          WireCodec.object(value).each do |key, entry|
            out << '<entry>'
            entry(out, target, key, entry)
            out << '</entry>'
          end
        else out << '>' << escape_xml(text(value), false)
        end
        out << '</' << name << '>'
      end
      def entry(out, map, key, value)
        info, key_xml, value_xml = shape(map), xml_member(map, 'key'), xml_member(map, 'value')
        element(out, key_xml.fetch('name'), key_xml['namespace'], info.fetch('key').fetch('target'), key)
        element(out, value_xml.fetch('name'), value_xml['namespace'], info.fetch('value').fetch('target'), value)
      end
      def child(out, owner, name, value)
        return if value.nil?
        xml = xml_member(owner, name)
        return if xml['attribute']
        target = member(owner, name).fetch('target')
        return element(out, xml.fetch('name'), xml['namespace'], target, value) unless xml['flattened']
        # A flattened collection repeats the member's own element, without a wrapper.
        info = shape(target)
        if info.fetch('kind') == 'list'
          namespace = xml['namespace'] || xml_member(target, 'member')['namespace']
          WireCodec.items(value).each { |entry| element(out, xml.fetch('name'), namespace, info.fetch('member').fetch('target'), entry) }
          return
        end
        WireCodec.object(value).each do |key, entry|
          out << '<' << xml.fetch('name')
          declare(out, xml['namespace'])
          out << '>'
          entry(out, target, key, entry)
          out << '</' << xml.fetch('name') << '>'
        end
      end
      # Scalar wire text, spelled as the codec's JSON would spell it.
      def text(value)
        case value
        when ::String then WireCodec.string(value)
        when JsonNumber then value.text
        when ::Integer, true, false then value.to_s
        when ::Float
          raise ::ArgumentError, 'Non-finite float' unless value.finite?
          value.to_s
        else raise ::ArgumentError, 'Expected an XML scalar'
        end
      end
      def escape_xml(text, attribute)
        text.each_char.map do |character|
          code = character.ord
          unless code == 0x9 || code == 0xA || code == 0xD || (0x20..0xD7FF).cover?(code) || (0xE000..0xFFFD).cover?(code) || (0x10000..0x10FFFF).cover?(code)
            raise ::ArgumentError, 'Text contains a character XML 1.0 cannot carry'
          end
          case character
          when '&' then '&amp;'
          when '<' then '&lt;'
          when '>' then '&gt;'
          # Character references survive end-of-line and attribute normalization.
          when "\r" then '&#xD;'
          when '"' then attribute ? '&quot;' : character
          when "\n" then attribute ? '&#xA;' : character
          when "\t" then attribute ? '&#x9;' : character
          else character
          end
        end.join
      end

      def members(pairs, prefix, target, values)
        ec2 = @protocol == 'ec2Query'
        shape(target).fetch('members').each_key do |name|
          value = values[name]
          next if value.nil?
          xml = xml_member(target, name)
          key = xml.fetch(ec2 ? 'queryName' : 'name')
          form_value(pairs, prefix.empty? ? key : prefix + '.' + key, member(target, name).fetch('target'), value, xml['flattened'] == true)
        end
      end
      def form_value(pairs, key, target, value, flattened)
        info = shape(target)
        # ec2Query always flattens collections and ignores their member names.
        ec2 = @protocol == 'ec2Query'
        flat = flattened || ec2
        case info.fetch('kind')
        when 'structure', 'union' then members(pairs, key, target, WireCodec.object(value))
        when 'list'
          items = WireCodec.items(value)
          # awsQuery spells an empty list as the bare key; ec2Query leaves it out.
          pairs << escape(key) + '=' if items.empty? && !ec2
          item_name = xml_member(target, 'member').fetch('name')
          items.each_with_index do |item, index|
            form_value(pairs, key + (flat ? '.' : '.' + item_name + '.') + (index + 1).to_s, info.fetch('member').fetch('target'), item, false)
          end
        when 'map'
          key_name, value_name = xml_member(target, 'key').fetch('name'), xml_member(target, 'value').fetch('name')
          WireCodec.object(value).each_with_index do |(entry_key, entry_value), index|
            base = key + (flat ? '.' : '.entry.') + (index + 1).to_s
            form_value(pairs, base + '.' + key_name, info.fetch('key').fetch('target'), entry_key, false)
            form_value(pairs, base + '.' + value_name, info.fetch('value').fetch('target'), entry_value, false)
          end
        else pairs << escape(key) + '=' + escape(text(value))
        end
      end
      def escape(text) = Endpoints.encode(text)

      # The flattened elements of a list or map, each one an item or an entry.
      def collection(nodes, target)
        info = shape(target)
        if info.fetch('kind') == 'list'
          item = info.fetch('member')
          return nodes.map { |node| read(node, item.fetch('target'), item['format']) }
        end
        nodes.to_h { |node| read_entry(node, target) }
      end
      def read_entry(node, map)
        info = shape(map)
        key = node.child(xml_member(map, 'key').fetch('name'))
        value = node.child(xml_member(map, 'value').fetch('name'))
        raise ::ArgumentError, 'XML map entry requires a key and a value' if key.nil? || value.nil?
        [key.text, read(value, info.fetch('value').fetch('target'), info.fetch('value')['format'])]
      end
      def read(node, target, format)
        info = shape(target)
        case info.fetch('kind')
        when 'structure', 'union'
          result = read_members(node, target, info.fetch('members').keys)
          if info.fetch('kind') == 'union' && result.empty? && (first = node.children.first)
            # An undeclared variant is kept by name, as its text.
            result[first.name] = first.text
          end
          result
        when 'list'
          item = info.fetch('member')
          node.all(xml_member(target, 'member').fetch('name')).map { |child| read(child, item.fetch('target'), item['format']) }
        when 'map' then node.all('entry').to_h { |child| read_entry(child, target) }
        else scalar(target, node.text, format)
        end
      end
      def scalar(target, text, format)
        info = shape(target)
        trimmed = text.strip
        case info.fetch('kind')
        when 'string', 'enum' then text
        when 'boolean'
          raise ::ArgumentError, 'Expected XML boolean' unless %w[true false].include?(trimmed)
          trimmed == 'true'
        when 'float', 'double' then NON_FINITE.include?(trimmed) ? trimmed : JsonNumber.new(trimmed)
        when 'byte', 'short', 'integer', 'long', 'bigInteger', 'bigDecimal', 'intEnum' then JsonNumber.new(trimmed)
        when 'timestamp' then (format || info['format']) == 'epoch-seconds' ? JsonNumber.new(trimmed) : trimmed
        when 'blob' then trimmed
        else raise ::ArgumentError, 'XML cannot carry ' + info.fetch('kind')
        end
      end
    end
  end
end
