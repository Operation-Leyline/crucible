# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # One permission a role grants (`resource_type`, the authorization
    # name grants carry, and `permission`, the value grants store).
    RolePermission = ::Data.define(:resource_type, :permission)
  end
end
