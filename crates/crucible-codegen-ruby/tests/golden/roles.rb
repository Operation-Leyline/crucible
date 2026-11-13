# frozen_string_literal: true
module Weather
  # The roles and scopes bindings store, and {ROLES}, the registry a role picker reads.
  module Roles
    # The service's roles, by the values bindings store, in catalogue order.
    class Role
      # @return [String] the value bindings store; a value this client does not list is kept as is
      attr_reader :value
      def initialize(value)
        @value = ::Weather::Runtime::WireCodec.string(value).freeze
        freeze
      end
      def ==(other) = other.is_a?(self.class) && @value == other.value
      alias eql? ==
      def hash = @value.hash
      # Runs the workspace.
      OWNER = new('owner')
      # Reads notes.
      VIEWER = new('viewer')
      # Reads, changes and writes notes.
      EDITOR = new('editor')
      # Edits notes and reads the archives they came from.
      ARCHIVIST = new('archivist')
      def inspect = 'Role(' + @value + ')'
      alias to_s inspect
    end
    # The service's scopes, by the values bindings store, in catalogue order.
    class Scope
      # @return [String] the value bindings store; a value this client does not list is kept as is
      attr_reader :value
      def initialize(value)
        @value = ::Weather::Runtime::WireCodec.string(value).freeze
        freeze
      end
      def ==(other) = other.is_a?(self.class) && @value == other.value
      alias eql? ==
      def hash = @value.hash
      # The workspace and everything in it.
      WORKSPACE = new('workspace')
      # Every note in the workspace, including notes written later.
      ALL_NOTES = new('allNotes')
      # One note.
      NOTE = new('note')
      def inspect = 'Scope(' + @value + ')'
      alias to_s inspect
    end
    # Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed.
    #
    # @return [Array<Runtime::RoleDescription>]
    ROLES = [
      ::Weather::Runtime::RoleDescription.new(
        value: 'owner',
        description: 'Runs the workspace.',
        scopes: ['workspace'].freeze,
        holders: ['user'].freeze,
        assigned_by: ['owner'].freeze,
        permissions: [::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:read'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:write'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:create'), ::Weather::Runtime::RolePermission.new(resource_type: 'workspace', permission: 'workspace:read'), ::Weather::Runtime::RolePermission.new(resource_type: 'workspace', permission: 'workspace:admin')].freeze,
      ),
      ::Weather::Runtime::RoleDescription.new(
        value: 'viewer',
        description: 'Reads notes.',
        scopes: ['workspace', 'allNotes', 'note'].freeze,
        holders: [].freeze,
        assigned_by: [].freeze,
        permissions: [::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:read')].freeze,
      ),
      ::Weather::Runtime::RoleDescription.new(
        value: 'editor',
        description: 'Reads, changes and writes notes.',
        scopes: ['allNotes', 'note'].freeze,
        holders: [].freeze,
        assigned_by: [].freeze,
        permissions: [::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:read'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:write'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:create')].freeze,
      ),
      ::Weather::Runtime::RoleDescription.new(
        value: 'archivist',
        description: 'Edits notes and reads the archives they came from.',
        scopes: ['allNotes'].freeze,
        holders: [].freeze,
        assigned_by: [].freeze,
        permissions: [::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:read'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:write'), ::Weather::Runtime::RolePermission.new(resource_type: 'note', permission: 'note:create')].freeze,
      ),
    ].freeze
  end
end
