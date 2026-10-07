package com.leyline.smithy.cargo;

import com.intellij.codeInsight.daemon.impl.AnnotationHolderImpl;
import com.intellij.lang.annotation.AnnotationSession;
import com.intellij.lang.annotation.Annotator;
import com.intellij.lang.annotation.HighlightSeverity;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.application.ApplicationStarter;
import com.intellij.openapi.project.DumbService;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.project.ProjectManager;
import com.intellij.openapi.vfs.LocalFileSystem;
import com.intellij.psi.PsiElement;
import com.intellij.psi.PsiFile;
import com.intellij.psi.PsiManager;
import com.intellij.psi.util.PsiTreeUtil;

import java.nio.file.Path;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/** Test-only application starter. Not included in the distributable plugin. */
public final class CargoSmoke implements ApplicationStarter {
    @Override public boolean isHeadless() { return true; }
    @Override public int getRequiredModality() { return NOT_IN_EDT; }

    @Override public void main(List<String> args) {
        try {
            Project project = ProjectManager.getInstance().loadAndOpenProject(args.get(1));
            if (project == null) throw new AssertionError("project did not open");
            CargoModels service = project.getService(CargoModels.class);
            service.refresh(false);
            long deadline = System.nanoTime() + 90_000_000_000L;
            while (service.libraryRoots().isEmpty() && System.nanoTime() < deadline) Thread.sleep(100);
            if (service.libraryRoots().isEmpty()) throw new AssertionError("Cargo model libraries did not load");
            DumbService.getInstance(project).waitForSmartMode();
            ApplicationManager.getApplication().runReadAction(() -> {
                var file = LocalFileSystem.getInstance().findFileByNioFile(Path.of(args.get(1), "smithy/hello.smithy"));
                PsiFile psi = PsiManager.getInstance(project).findFile(file);
                if (psi == null) throw new AssertionError("Smithy PSI did not load");
                Set<String> resolved = new LinkedHashSet<>();
                for (PsiElement element : PsiTreeUtil.collectElements(psi, ignored -> true)) {
                    for (var reference : element.getReferences()) {
                        if (Set.of("String", "json", "ValidationError").contains(element.getText())) {
                            var target = reference.resolve();
                            if (target == null) throw new AssertionError("unresolved " + element.getText());
                            resolved.add(element.getText());
                        }
                    }
                }
                if (!resolved.containsAll(Set.of("String", "json", "ValidationError"))) {
                    throw new AssertionError("expected navigation references were not checked: " + resolved);
                }
                try {
                    Annotator annotator = (Annotator) Class.forName("software.amazon.smithy.intellij.SmithyAnnotator").getConstructor().newInstance();
                    var annotations = new AnnotationHolderImpl(new AnnotationSession(psi), false);
                    for (PsiElement element : PsiTreeUtil.collectElements(psi, ignored -> true)) {
                        annotations.runAnnotatorWithContext(element, annotator);
                    }
                    var errors = annotations.stream().filter(annotation -> annotation.getSeverity() == HighlightSeverity.ERROR).map(annotation -> annotation.getMessage()).toList();
                    if (!errors.isEmpty()) throw new AssertionError("Smithy editor errors: " + errors);
                } catch (ReflectiveOperationException error) {
                    throw new AssertionError(error);
                }
            });
            System.out.println("SMITHY_CARGO_SMOKE_OK: String, json and ValidationError resolve; valid model has no error annotations");
            System.exit(0);
        } catch (Throwable error) {
            error.printStackTrace();
            System.exit(1);
        }
    }
}
