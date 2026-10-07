package com.leyline.smithy.cargo;

import com.intellij.openapi.project.Project;
import com.intellij.openapi.roots.AdditionalLibraryRootsProvider;
import com.intellij.openapi.roots.SyntheticLibrary;
import com.intellij.openapi.vfs.VirtualFile;

import java.util.Collection;
import java.util.List;

public final class CargoLibraries extends AdditionalLibraryRootsProvider {
    @Override
    public Collection<SyntheticLibrary> getAdditionalProjectLibraries(Project project) {
        List<VirtualFile> roots = project.getService(CargoModels.class).libraryRoots();
        return roots.isEmpty() ? List.of() : List.of(SyntheticLibrary.newImmutableLibrary(roots));
    }

    @Override
    public Collection<VirtualFile> getRootsToWatch(Project project) {
        return project.getService(CargoModels.class).libraryRoots();
    }
}
