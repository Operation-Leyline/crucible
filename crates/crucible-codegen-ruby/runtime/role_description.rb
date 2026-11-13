# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # One role in the client's role registry (`Roles::ROLES`).
    #
    # `value` is what bindings store; `description` the model's
    # documentation for it, empty when it has none; `scopes` the scope
    # values it may be bound at, in catalogue order; `holders` the kinds of
    # principal that may hold it, empty for any; `assigned_by` the roles
    # whose holders alone may assign it, empty for whoever may write
    # bindings; `permissions` what it grants, each a {RolePermission}, only
    # the permissions this client knows.
    RoleDescription = ::Data.define(:value, :description, :scopes, :holders, :assigned_by, :permissions)
  end
end
