# frozen_string_literal: true
module Weather
  # The permissions grants store, by resource type, and {PERMISSIONS}, the registry a grant picker reads.
  module Permissions
    # The permissions `Graph` accepts (`graph`), by the values grants store.
    class GraphPermission
      # @return [String] the value grants store; a value this client does not list is kept as is
      attr_reader :value
      def initialize(value)
        @value = ::Weather::Runtime::WireCodec.string(value).freeze
        freeze
      end
      def ==(other) = other.is_a?(self.class) && @value == other.value
      alias eql? ==
      def hash = @value.hash
      # Read a graph and its catalog.
      READ = new('graph:read')
      # Change a graph: */ ends no comment and \[bare] links nowhere.
      # \@param is not a tag.
      WRITE = new('graph:write')
      AUDIT = new('graph:audit')
      # An alias of `graph:read`.
      #
      # Deprecated: Renamed to graph:read.
      #
      # @deprecated Renamed to graph:read.
      VIEW = new('graph:view')
      # The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.
      #
      # @return [GraphPermission]
      def canonical
        return READ if self == VIEW
        self
      end
      def inspect = 'GraphPermission(' + @value + ')'
      alias to_s inspect
    end
    # The permissions `History` accepts (`history`), by the values grants store.
    class HistoryPermission
      # @return [String] the value grants store; a value this client does not list is kept as is
      attr_reader :value
      def initialize(value)
        @value = ::Weather::Runtime::WireCodec.string(value).freeze
        freeze
      end
      def ==(other) = other.is_a?(self.class) && @value == other.value
      alias eql? ==
      def hash = @value.hash
      READ = new('history:read')
      RESTORE = new('history:restore')
      # The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.
      #
      # @return [HistoryPermission]
      def canonical
        self
      end
      def inspect = 'HistoryPermission(' + @value + ')'
      alias to_s inspect
    end
    # The permissions `Organization` accepts (`org`), by the values grants store.
    class OrganizationPermission
      # @return [String] the value grants store; a value this client does not list is kept as is
      attr_reader :value
      def initialize(value)
        @value = ::Weather::Runtime::WireCodec.string(value).freeze
        freeze
      end
      def ==(other) = other.is_a?(self.class) && @value == other.value
      alias eql? ==
      def hash = @value.hash
      # Everything in the organization.
      ADMIN = new('organization:admin')
      # The permission this one stands for: a deprecated alias's primary, or this one, unknown values included.
      #
      # @return [OrganizationPermission]
      def canonical
        self
      end
      def inspect = 'OrganizationPermission(' + @value + ')'
      alias to_s inspect
    end
    # Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the `grantable` ones that are not `deprecated`.
    #
    # @return [Array<Runtime::ResourcePermissions>]
    PERMISSIONS = [
      ::Weather::Runtime::ResourcePermissions.new(resource_type: 'graph', permissions: [
        ::Weather::Runtime::PermissionDescription.new(value: 'graph:audit', description: '', tier: 'permissionOnly', grantable: true, deprecated: false),
        ::Weather::Runtime::PermissionDescription.new(value: 'graph:read', description: 'Read a graph and its catalog.', tier: 'public', grantable: true, deprecated: false),
        ::Weather::Runtime::PermissionDescription.new(value: 'graph:view', description: '', tier: 'public', grantable: true, deprecated: true, alias_of: 'graph:read'),
        ::Weather::Runtime::PermissionDescription.new(value: 'graph:write', description: "Change a graph: */ ends no comment and [bare] links nowhere.\n@param is not a tag.", tier: 'public', grantable: true, deprecated: false),
      ].freeze),
      ::Weather::Runtime::ResourcePermissions.new(resource_type: 'history', permissions: [
        ::Weather::Runtime::PermissionDescription.new(value: 'history:read', description: '', tier: 'public', grantable: true, deprecated: false),
        ::Weather::Runtime::PermissionDescription.new(value: 'history:restore', description: '', tier: 'permissionOnly', grantable: true, deprecated: false),
      ].freeze),
      ::Weather::Runtime::ResourcePermissions.new(resource_type: 'org', permissions: [
        ::Weather::Runtime::PermissionDescription.new(value: 'organization:admin', description: 'Everything in the organization.', tier: 'notGrantable', grantable: false, deprecated: false),
      ].freeze),
    ].freeze
  end
end
