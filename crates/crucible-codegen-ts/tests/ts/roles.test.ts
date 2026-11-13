import assert from "node:assert/strict";
import { test } from "node:test";
import {
  NotesRole,
  type NotesRoleValue,
  NotesScope,
  type NotesScopeValue,
  ROLES,
  type RoleDescription,
  type RolePermission,
} from "./generated-roles/src/index.js";

// tests/models/roles-granted-by, generated for the server audience: the
// registry is the client's all the same (docs/roles.md §7 "Clients").

test("the registry lists public roles in catalogue order with their flags", () => {
  assert.deepEqual(ROLES.map(role => role.value), ["owner", "viewer", "editor", "archivist"]);
  const owner: RoleDescription = ROLES[0];
  assert.equal(owner.value, NotesRole.OWNER);
  assert.deepEqual(owner.scopes, [NotesScope.WORKSPACE]);
  assert.deepEqual(owner.holders, ["user"]);
  assert.deepEqual(owner.assignedBy, [NotesRole.OWNER]);
  assert.equal(owner.description, "Runs the workspace.");
  const viewer = ROLES[1];
  assert.deepEqual(viewer.scopes, ["workspace", "allNotes", "note"]);
  assert.deepEqual(viewer.holders, []);
  assert.deepEqual(viewer.assignedBy, []);
});

test("a role lists only the public permissions it grants", () => {
  const owner = ROLES[0];
  const granted: RolePermission[] = [...owner.permissions];
  assert.deepEqual(
    granted.map(p => `${p.resourceType} ${p.permission}`),
    ["note note:read", "note note:write", "note note:create", "workspace workspace:read", "workspace workspace:admin"],
  );
  const values = ROLES.flatMap(role => role.permissions.map(p => p.permission));
  assert.equal(values.includes("note:purge"), false);
  assert.equal(values.includes("workspace:audit"), false);
  // A `by service` grant is the service's to decide, never listed.
  assert.equal(ROLES[3].permissions.some(p => p.permission === "archive:read"), false);
});

test("an internal role and scope are absent, and unknown values round-trip", () => {
  assert.equal(ROLES.some(role => role.value === "auditor"), false);
  assert.deepEqual(Object.values(NotesScope), ["workspace", "allNotes", "note"]);
  // A binding written by a newer server, a custom role, or naming an
  // internal role or scope is still a value of the type.
  const roles: NotesRoleValue[] = JSON.parse('["viewer","auditor","custom/reviewer"]');
  const scopes: NotesScopeValue[] = JSON.parse('["note","backOffice"]');
  assert.deepEqual(roles, ["viewer", "auditor", "custom/reviewer"]);
  assert.equal(roles[0], NotesRole.VIEWER);
  assert.equal(scopes[1], "backOffice");
});
