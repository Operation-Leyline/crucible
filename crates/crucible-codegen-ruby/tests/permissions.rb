# frozen_string_literal: true
# Run against a gem generated from tests/fixtures/permissions with module
# Leyline: ruby -I <gem>/lib permissions.rb. The open permission classes and
# the registry as an application reads them (docs/permissions.md §6.3).
require 'json'
require 'leyline_client'

def check(condition, message)
  raise message unless condition
end

permissions = Leyline::Permissions
graph = permissions::GraphPermission

# A grant written by a newer server, or naming an internal permission this
# client never sees, is still a value of the type, kept as it is.
stored = JSON.parse('["graph:read","graph:export","organization:read"]').map { |value| graph.new(value) }
check(stored[0] == graph::READ, 'a known value is its constant')
check(graph::READ.value == 'graph:read', 'a constant carries the value grants store')
check(stored.map(&:value) == ['graph:read', 'graph:export', 'organization:read'], 'unknown values are kept')
check(JSON.generate(stored.map(&:value)) == '["graph:read","graph:export","organization:read"]', 'unknown values are written back unchanged')
check(stored[1].canonical.equal?(stored[1]), 'an unknown value is its own canonical form')

# A deprecated alias keeps its value and canonicalizes to its primary.
check(graph::VIEW.value == 'graph:view', 'the alias keeps its value')
check(graph::VIEW.canonical == graph::READ, 'the alias canonicalizes to its primary')
check(graph.new('graph:view').canonical == graph::READ, 'a parsed alias canonicalizes too')
check(graph::READ.canonical == graph::READ, 'a primary is its own canonical form')
check(permissions::HistoryPermission::RESTORE.canonical == permissions::HistoryPermission::RESTORE, 'no alias, no change')
check({ graph::READ => 1 }.key?(graph.new('graph:read')), 'equal values hash alike')

# The registry offers grantable permissions and no internal one.
registry = permissions::PERMISSIONS
check(registry.map(&:resource_type) == %w[graph history org], "resource types: #{registry.map(&:resource_type)}")
values = registry.flat_map { |resource| resource.permissions.map(&:value) }
check(!values.include?('organization:read'), 'no internal permission in the registry')
check(!permissions::OrganizationPermission.constants.map { |name| permissions::OrganizationPermission.const_get(name) }
        .any? { |constant| constant.respond_to?(:value) && constant.value == 'organization:read' }, 'no internal constant')
admin = registry[2].permissions.find { |p| p.value == permissions::OrganizationPermission::ADMIN.value }
check(admin.grantable == false && admin.tier == 'notGrantable', 'a bundle is not grantable')
view = registry[0].permissions.find { |p| p.value == 'graph:view' }
check(view.alias_of == 'graph:read' && view.deprecated == true, 'the alias names its primary and is deprecated')
check(registry[0].permissions.find { |p| p.value == 'graph:read' }.alias_of.nil?, 'a primary has no alias_of')
check(registry.frozen? && registry[0].permissions.frozen? && registry[0].frozen?, 'the registry is immutable')
picker = registry.flat_map { |resource| resource.permissions.select { |p| p.grantable && !p.deprecated } }
check(picker.map(&:value) == ['graph:audit', 'graph:read', 'graph:write', 'history:read', 'history:restore'],
      "picker: #{picker.map(&:value)}")
puts 'Ruby permissions passed'
