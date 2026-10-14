# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # One permission in the client's permission registry
    # (`Permissions::PERMISSIONS`).
    #
    # `value` is what grants store; `description` the model's documentation
    # for it, empty when it has none; `tier` is `'public'`,
    # `'permissionOnly'` or `'notGrantable'`, a plain string since a newer
    # server may add tiers; `grantable` is false for a bundle the service
    # confers whole, which a grant picker never offers; `alias_of` is, for a
    # deprecated alias, the value it stands for, and nil otherwise.
    PermissionDescription = ::Data.define(:value, :description, :tier, :grantable, :deprecated, :alias_of) do
      def initialize(value:, description:, tier:, grantable:, deprecated:, alias_of: nil)
        super
      end
    end
  end
end
