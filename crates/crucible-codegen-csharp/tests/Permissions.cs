using Example.Leyline.Permissions;

// The typed permissions fixture's registry and open permission records, as a consumer reads them.
static class Program
{
    static void Check(bool value, string message) { if (!value) throw new Exception(message); }
    static void Equal(object? expected, object? actual) => Check(Equals(expected, actual), $"expected <{expected}> but was <{actual}>");

    static PermissionDescription Find(string resourceType, string value) =>
        PermissionRegistry.Permissions.Single(r => r.ResourceType == resourceType).Permissions.Single(p => p.Value == value);

    static void Main()
    {
        Equal("graph:read", GraphPermission.Read.Value);
        Equal("graph:read", GraphPermission.Read.ToString());
        Equal("history:restore", HistoryPermission.Restore.Value);
        Equal("organization:admin", OrganizationPermission.Admin.Value);
        Equal(GraphPermission.Read, new GraphPermission("graph:read"));
        // Open: a value this client does not know, a newer server's or an internal one's, survives unchanged.
        foreach (var value in new[] { "graph:export", "organization:read" })
        {
            var unknown = new GraphPermission(value);
            Equal(value, unknown.Value);
            Equal(value, unknown.ToString());
            Equal(unknown, unknown.Canonical());
        }
#pragma warning disable CS0618
        Equal(GraphPermission.Read, GraphPermission.View.Canonical());
#pragma warning restore CS0618
        Equal(GraphPermission.Read, new GraphPermission("graph:view").Canonical());
        Equal(GraphPermission.Write, GraphPermission.Write.Canonical());
        Equal(OrganizationPermission.Admin, OrganizationPermission.Admin.Canonical());

        Equal("graph,history,org", string.Join(",", PermissionRegistry.Permissions.Select(r => r.ResourceType)));
        Equal("graph:audit,graph:read,graph:view,graph:write", string.Join(",", PermissionRegistry.Permissions[0].Permissions.Select(p => p.Value)));
        var admin = Find("org", "organization:admin");
        Check(!admin.Grantable, "organization:admin is not grantable");
        Equal("notGrantable", admin.Tier);
        var view = Find("graph", "graph:view");
        Equal("graph:read", view.AliasOf);
        Check(view.Deprecated, "graph:view is deprecated");
        var read = Find("graph", "graph:read");
        Check(read.AliasOf is null && !read.Deprecated && read.Grantable, "graph:read is a grantable primary");
        Equal("public", read.Tier);
        Equal("Read a graph and its catalog.", read.Description);
        Equal("permissionOnly", Find("graph", "graph:audit").Tier);
        Check(PermissionRegistry.Permissions.SelectMany(r => r.Permissions).All(p => p.Value != "organization:read"), "an internal permission is listed");
        // A grant picker offers the grantable permissions that are not deprecated.
        var offered = PermissionRegistry.Permissions.SelectMany(r => r.Permissions).Where(p => p.Grantable && !p.Deprecated).Select(p => p.Value);
        Equal("graph:audit,graph:read,graph:write,history:read,history:restore", string.Join(",", offered));
        Console.WriteLine("C# permissions consumer passed");
    }
}
