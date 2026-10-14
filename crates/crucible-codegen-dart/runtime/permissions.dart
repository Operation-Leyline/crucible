/// One permission in a client's registry, the `permissions` constant of
/// `permissions.dart`.
final class PermissionDescription {
  /// What grants store.
  final String value;

  /// The model's documentation for the permission, empty when it has none.
  final String description;

  /// `'public'`, `'permissionOnly'` or `'notGrantable'`: a plain string, since
  /// a newer server may add tiers.
  final String tier;

  /// False for a bundle the service confers whole: never offer it in a picker.
  final bool grantable;

  /// True for a deprecated permission, every alias included.
  final bool deprecated;

  /// For a deprecated alias, the value it stands for.
  final String? aliasOf;

  const PermissionDescription({
    required this.value,
    required this.description,
    required this.tier,
    required this.grantable,
    required this.deprecated,
    this.aliasOf,
  });

  /// The entry as JSON, under the field names a service description's
  /// registry uses, so `jsonEncode` can serve it to a console.
  Map<String, Object?> toJson() => {
        'value': value,
        'description': description,
        'tier': tier,
        'grantable': grantable,
        'deprecated': deprecated,
        if (aliasOf != null) 'aliasOf': aliasOf,
      };

  @override
  String toString() => 'PermissionDescription($value)';
}

/// One resource type's permissions, sorted by value.
final class ResourcePermissions {
  /// The authorization name grants use for the resource type.
  final String resourceType;

  /// The type's permissions, deprecated aliases included.
  final List<PermissionDescription> permissions;

  const ResourcePermissions({
    required this.resourceType,
    required this.permissions,
  });

  /// The resource type as JSON, under the field names a service
  /// description's registry uses.
  Map<String, Object?> toJson() => {
        'resourceType': resourceType,
        'permissions': [for (final permission in permissions) permission.toJson()],
      };

  @override
  String toString() => 'ResourcePermissions($resourceType)';
}
