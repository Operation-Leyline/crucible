package example.leyline;

import example.leyline.permissions.GraphPermission;
import example.leyline.permissions.HistoryPermission;
import example.leyline.permissions.OrganizationPermission;
import example.leyline.permissions.PermissionDescription;
import example.leyline.permissions.Permissions;
import example.leyline.permissions.ResourcePermissions;
import java.util.List;
import java.util.stream.Collectors;

/** The typed permissions fixture's registry and open permission types, as a consumer reads them. */
public final class PermissionsTest {
  static void check(boolean ok, String message) { if (!ok) throw new AssertionError(message); }

  static void equal(Object expected, Object actual) {
    check(expected.equals(actual), "expected <" + expected + "> but was <" + actual + ">");
  }

  static PermissionDescription find(String resourceType, String value) {
    return Permissions.PERMISSIONS.stream().filter(r -> r.resourceType().equals(resourceType))
        .flatMap(r -> r.permissions().stream()).filter(p -> p.value().equals(value)).findFirst()
        .orElseThrow(() -> new AssertionError("no " + value + " on " + resourceType));
  }

  @SuppressWarnings("deprecation")
  public static void main(String[] args) {
    equal("graph:read", GraphPermission.READ.value());
    equal("graph:read", GraphPermission.READ.toString());
    equal("history:restore", HistoryPermission.RESTORE.value());
    equal("organization:admin", OrganizationPermission.ADMIN.value());
    // Open: a value this client does not know, a newer server's or an internal one's, survives unchanged.
    for (String value : List.of("graph:export", "organization:read")) {
      GraphPermission unknown = new GraphPermission(value);
      equal(value, unknown.value());
      equal(value, unknown.toString());
      equal(unknown, unknown.canonical());
    }
    equal(new GraphPermission("graph:read"), GraphPermission.READ);
    equal(GraphPermission.READ, GraphPermission.VIEW.canonical());
    equal(GraphPermission.READ, new GraphPermission("graph:view").canonical());
    equal(GraphPermission.WRITE, GraphPermission.WRITE.canonical());
    equal(OrganizationPermission.ADMIN, OrganizationPermission.ADMIN.canonical());

    equal(List.of("graph", "history", "org"),
        Permissions.PERMISSIONS.stream().map(ResourcePermissions::resourceType).collect(Collectors.toList()));
    equal(List.of("graph:audit", "graph:read", "graph:view", "graph:write"),
        Permissions.PERMISSIONS.get(0).permissions().stream().map(PermissionDescription::value).collect(Collectors.toList()));
    PermissionDescription admin = find("org", "organization:admin");
    check(!admin.grantable(), "organization:admin is not grantable");
    equal("notGrantable", admin.tier());
    PermissionDescription view = find("graph", "graph:view");
    equal("graph:read", view.aliasOf());
    check(view.deprecated(), "graph:view is deprecated");
    PermissionDescription read = find("graph", "graph:read");
    check(read.aliasOf() == null && !read.deprecated() && read.grantable(), "graph:read is a grantable primary");
    equal("public", read.tier());
    equal("Read a graph and its catalog.", read.description());
    equal("permissionOnly", find("graph", "graph:audit").tier());
    for (ResourcePermissions resource : Permissions.PERMISSIONS)
      for (PermissionDescription permission : resource.permissions())
        check(!permission.value().equals("organization:read"), "an internal permission is listed");
    // A grant picker offers the grantable permissions that are not deprecated.
    List<String> offered = Permissions.PERMISSIONS.stream().flatMap(r -> r.permissions().stream())
        .filter(p -> p.grantable() && !p.deprecated()).map(PermissionDescription::value).collect(Collectors.toList());
    equal(List.of("graph:audit", "graph:read", "graph:write", "history:read", "history:restore"), offered);
    System.out.println("Java permissions consumer passed");
  }
}
