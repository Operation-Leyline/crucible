/// One permission a role grants, in a client's role registry.
final class RolePermission {
  /// The authorization name grants use for the resource type.
  final String resourceType;

  /// What grants store.
  final String permission;

  const RolePermission({
    required this.resourceType,
    required this.permission,
  });

  /// The entry as JSON, under the field names a service description's
  /// registry uses.
  Map<String, Object?> toJson() => {
        'resourceType': resourceType,
        'permission': permission,
      };

  @override
  String toString() => 'RolePermission($resourceType, $permission)';
}

/// One role in a client's registry, the `roles` constant of `roles.dart`.
final class RoleDescription {
  /// What bindings store.
  final String value;

  /// The model's documentation for the role, empty when it has none.
  final String description;

  /// The scope values the role may be bound at, in catalogue order.
  final List<String> scopes;

  /// The kinds of principal that may hold the role; empty: any.
  final List<String> holders;

  /// The roles whose holders alone may assign the role; empty: whoever may
  /// write bindings.
  final List<String> assignedBy;

  /// What the role grants: the permissions this client knows.
  final List<RolePermission> permissions;

  const RoleDescription({
    required this.value,
    required this.description,
    required this.scopes,
    required this.holders,
    required this.assignedBy,
    required this.permissions,
  });

  /// The entry as JSON, under the field names a service description's
  /// registry uses, so `jsonEncode` can serve it to a console.
  Map<String, Object?> toJson() => {
        'value': value,
        'description': description,
        'scopes': scopes,
        'holders': holders,
        'assignedBy': assignedBy,
        'permissions': [for (final permission in permissions) permission.toJson()],
      };

  @override
  String toString() => 'RoleDescription($value)';
}
