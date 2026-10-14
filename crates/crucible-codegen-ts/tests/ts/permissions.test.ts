import assert from "node:assert/strict";
import { test } from "node:test";
import {
  GraphPermission,
  type GraphPermissionValue,
  OrganizationPermission,
  PERMISSIONS,
  type PermissionDescription,
  type ResourcePermissions,
  canonicalPermission,
} from "./generated-permissions/src/index.js";

test("permission values a client does not know round-trip unchanged", () => {
  // A grant written by a newer server, or naming an internal permission
  // this client never sees, is still a value of the type.
  const stored: GraphPermissionValue[] = JSON.parse('["graph:read","graph:export","organization:read"]');
  assert.deepEqual(stored, ["graph:read", "graph:export", "organization:read"]);
  assert.equal(JSON.stringify(stored), '["graph:read","graph:export","organization:read"]');
  assert.equal(stored[0], GraphPermission.READ);
  assert.equal(canonicalPermission("graph", "graph:export"), "graph:export");
});

test("a deprecated alias keeps its value and canonicalizes to its primary", () => {
  assert.equal(GraphPermission.VIEW, "graph:view");
  assert.equal(canonicalPermission("graph", GraphPermission.VIEW), GraphPermission.READ);
  assert.equal(canonicalPermission("history", GraphPermission.VIEW), GraphPermission.VIEW);
});

test("the registry offers grantable public permissions and no internal one", () => {
  assert.deepEqual(PERMISSIONS.map(resource => resource.resourceType), ["graph", "history", "org"]);
  const values = PERMISSIONS.flatMap(resource => resource.permissions.map(p => p.value));
  assert.equal(values.includes("organization:read"), false);
  const admin = PERMISSIONS[2].permissions.find(p => p.value === OrganizationPermission.ADMIN);
  assert.equal(admin?.grantable, false);
  assert.equal(admin?.tier, "notGrantable");
  const alias = PERMISSIONS[0].permissions.find(p => p.value === "graph:view");
  assert.equal(alias?.aliasOf, "graph:read");
  assert.equal(alias?.deprecated, true);
  // The registry's types are exported by name from the package root.
  const graph: ResourcePermissions = PERMISSIONS[0];
  const picker: PermissionDescription[] = PERMISSIONS.flatMap(resource => resource.permissions.filter(p => p.grantable && !p.deprecated));
  assert.equal(graph.resourceType, "graph");
  assert.deepEqual(picker.map(p => p.value), ["graph:audit", "graph:read", "graph:write", "history:read", "history:restore"]);
});
