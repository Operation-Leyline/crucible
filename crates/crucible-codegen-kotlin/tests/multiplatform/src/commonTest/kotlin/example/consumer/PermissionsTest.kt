package example.consumer

import example.leyline.permissions.GraphPermission
import example.leyline.permissions.HistoryPermission
import example.leyline.permissions.OrganizationPermission
import example.leyline.permissions.PERMISSIONS
import example.leyline.permissions.PermissionDescription
import kotlin.test.*

// The typed permissions fixture's registry and open permission types, on every target.
class PermissionsTest {
    private fun find(resourceType: String, value: String): PermissionDescription =
        PERMISSIONS.single { it.resourceType == resourceType }.permissions.single { it.value == value }

    @Test fun constantsAreTheValuesGrantsStore() {
        assertEquals("graph:read", GraphPermission.READ.value)
        assertEquals("graph:read", GraphPermission.READ.toString())
        assertEquals("history:restore", HistoryPermission.RESTORE.value)
        assertEquals("organization:admin", OrganizationPermission.ADMIN.value)
        assertEquals(GraphPermission.READ, GraphPermission("graph:read"))
    }

    @Test fun unknownValuesSurviveUnchanged() {
        for (value in listOf("graph:export", "organization:read")) {
            val unknown = GraphPermission(value)
            assertEquals(value, unknown.value)
            assertEquals(value, unknown.toString())
            assertEquals(unknown, unknown.canonical())
        }
    }

    @Suppress("DEPRECATION")
    @Test fun canonicalMapsAnAliasToItsPrimary() {
        assertEquals(GraphPermission.READ, GraphPermission.VIEW.canonical())
        assertEquals(GraphPermission.READ, GraphPermission("graph:view").canonical())
        assertEquals(GraphPermission.WRITE, GraphPermission.WRITE.canonical())
        assertEquals(OrganizationPermission.ADMIN, OrganizationPermission.ADMIN.canonical())
    }

    @Test fun registryListsWhatAConsoleOffers() {
        assertEquals(listOf("graph", "history", "org"), PERMISSIONS.map { it.resourceType })
        assertEquals(listOf("graph:audit", "graph:read", "graph:view", "graph:write"), PERMISSIONS[0].permissions.map { it.value })
        val admin = find("org", "organization:admin")
        assertFalse(admin.grantable)
        assertEquals("notGrantable", admin.tier)
        val view = find("graph", "graph:view")
        assertEquals("graph:read", view.aliasOf)
        assertTrue(view.deprecated)
        val read = find("graph", "graph:read")
        assertNull(read.aliasOf)
        assertFalse(read.deprecated)
        assertEquals("public", read.tier)
        assertEquals("Read a graph and its catalog.", read.description)
        assertEquals("permissionOnly", find("graph", "graph:audit").tier)
        assertTrue(PERMISSIONS.flatMap { it.permissions }.none { it.value == "organization:read" })
        val offered = PERMISSIONS.flatMap { it.permissions }.filter { it.grantable && !it.deprecated }.map { it.value }
        assertEquals(listOf("graph:audit", "graph:read", "graph:write", "history:read", "history:restore"), offered)
    }
}
