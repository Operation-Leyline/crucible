package com.leyline.smithy.cargo;

import com.intellij.openapi.project.Project;
import com.intellij.openapi.vfs.VirtualFile;
import com.intellij.psi.ResolveScopeProvider;
import com.intellij.psi.search.GlobalSearchScope;
import com.intellij.psi.search.GlobalSearchScopesCore;

import java.util.List;

public final class CargoScope extends ResolveScopeProvider {
    @Override
    public GlobalSearchScope getResolveScope(VirtualFile file, Project project) {
        if (!"smithy".equals(file.getExtension())) return null;
        List<VirtualFile> roots = project.getService(CargoModels.class).scope(file.toNioPath());
        if (roots.isEmpty()) return null;
        // Workspace members can select different compiler versions. Their resolve
        // scopes must not combine both preludes simply because both are indexed.
        GlobalSearchScope scope = GlobalSearchScope.EMPTY_SCOPE;
        for (VirtualFile root : roots) {
            scope = scope.uniteWith(root.isDirectory()
                    ? GlobalSearchScopesCore.directoryScope(project, root, true)
                    : GlobalSearchScope.fileScope(project, root));
        }
        return scope;
    }
}
