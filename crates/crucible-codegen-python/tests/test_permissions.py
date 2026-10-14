"""Typed permissions: a consumer of the client generated from tests/fixtures/permissions.

The registry and the open permission types as an application reads them
(docs/permissions.md §6.3); `tools/test-python.py` generates the package as
`leyline_client`.
"""
import json
import unittest

from leyline_client.permissions import (PERMISSIONS, GraphPermission, HistoryPermission,
                                        OrganizationPermission, PermissionDescription,
                                        ResourcePermissions)


class PermissionsTest(unittest.TestCase):
    def test_values_a_client_does_not_know_round_trip_unchanged(self) -> None:
        # A grant written by a newer server, or naming an internal permission
        # this client never sees, is still a value of the type.
        stored = [GraphPermission(value) for value in json.loads('["graph:read","graph:export","organization:read"]')]
        self.assertIs(stored[0], GraphPermission.READ)
        self.assertEqual(stored[0], "graph:read")
        self.assertEqual([value.value for value in stored], ["graph:read", "graph:export", "organization:read"])
        self.assertEqual(json.dumps(stored), '["graph:read", "graph:export", "organization:read"]')
        self.assertIs(stored[1].canonical(), stored[1])
        self.assertEqual(stored[1].canonical(), "graph:export")

    def test_a_deprecated_alias_keeps_its_value_and_canonicalizes_to_its_primary(self) -> None:
        self.assertEqual(GraphPermission.VIEW.value, "graph:view")
        self.assertIs(GraphPermission.VIEW.canonical(), GraphPermission.READ)
        self.assertIs(GraphPermission("graph:view").canonical(), GraphPermission.READ)
        self.assertIs(GraphPermission.READ.canonical(), GraphPermission.READ)
        self.assertIs(HistoryPermission.RESTORE.canonical(), HistoryPermission.RESTORE)

    def test_the_registry_offers_grantable_permissions_and_no_internal_one(self) -> None:
        self.assertEqual([resource.resource_type for resource in PERMISSIONS], ["graph", "history", "org"])
        values = [p.value for resource in PERMISSIONS for p in resource.permissions]
        self.assertNotIn("organization:read", values)
        self.assertNotIn("organization:read", [member.value for member in OrganizationPermission])
        admin = next(p for p in PERMISSIONS[2].permissions if p.value == OrganizationPermission.ADMIN)
        self.assertFalse(admin.grantable)
        self.assertEqual(admin.tier, "notGrantable")
        alias = next(p for p in PERMISSIONS[0].permissions if p.value == "graph:view")
        self.assertEqual(alias.alias_of, "graph:read")
        self.assertTrue(alias.deprecated)
        self.assertIsNone(PERMISSIONS[0].permissions[1].alias_of)
        graph: ResourcePermissions = PERMISSIONS[0]
        picker: list[PermissionDescription] = [
            p for resource in PERMISSIONS for p in resource.permissions if p.grantable and not p.deprecated
        ]
        self.assertEqual(graph.resource_type, "graph")
        self.assertEqual([p.value for p in picker],
                         ["graph:audit", "graph:read", "graph:write", "history:read", "history:restore"])


if __name__ == "__main__":
    unittest.main()
