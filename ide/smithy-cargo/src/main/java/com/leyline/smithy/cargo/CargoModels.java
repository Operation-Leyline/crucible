package com.leyline.smithy.cargo;

import com.intellij.ProjectTopics;
import com.intellij.execution.configurations.GeneralCommandLine;
import com.intellij.execution.process.CapturingProcessHandler;
import com.intellij.ide.util.PropertiesComponent;
import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer;
import com.intellij.psi.PsiManager;
import com.intellij.notification.NotificationGroupManager;
import com.intellij.notification.NotificationType;
import com.intellij.openapi.Disposable;
import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.components.Service;
import com.intellij.openapi.diagnostic.Logger;
import com.intellij.openapi.module.ModuleManager;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.roots.AdditionalLibraryRootsListener;
import com.intellij.openapi.roots.ModuleRootModificationUtil;
import com.intellij.openapi.roots.ModuleRootListener;
import com.intellij.openapi.roots.ModuleRootEvent;
import com.intellij.openapi.vfs.LocalFileSystem;
import com.intellij.openapi.vfs.VirtualFile;
import com.intellij.openapi.vfs.VirtualFileManager;
import com.intellij.openapi.vfs.newvfs.BulkFileListener;
import com.intellij.openapi.vfs.newvfs.events.VFileEvent;
import com.intellij.util.Alarm;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.atomic.AtomicBoolean;

@Service(Service.Level.PROJECT)
public final class CargoModels implements Disposable {
    private static final Logger LOG = Logger.getInstance(CargoModels.class);
    private static final String SOURCE_KEY = "smithy.cargo.ownedSources";
    private final Project project;
    private final Alarm alarm;
    private final AtomicBoolean running = new AtomicBoolean();
    private final AtomicBoolean again = new AtomicBoolean();
    private volatile boolean closed;
    private volatile boolean applying;
    private volatile CapturingProcessHandler process;
    private volatile State state = new State(null, Map.of(), List.of());

    private record State(CargoMetadata metadata, Map<Path, VirtualFile> files, List<VirtualFile> libraries) {}

    public CargoModels(Project project) {
        this.project = project;
        this.alarm = new Alarm(Alarm.ThreadToUse.POOLED_THREAD, this);
        var connection = project.getMessageBus().connect(this);
        connection.subscribe(VirtualFileManager.VFS_CHANGES, new BulkFileListener() {
            @Override public void after(List<? extends VFileEvent> events) {
                if (project.getBasePath() == null) return;
                Path base = Path.of(project.getBasePath());
                for (VFileEvent event : events) {
                    Path path = Path.of(event.getPath());
                    String name = path.getFileName().toString();
                    if (path.startsWith(base) && (name.equals("Cargo.toml") || name.equals("Cargo.lock"))) {
                        refresh(false);
                        break;
                    }
                }
            }
        });
        connection.subscribe(ProjectTopics.PROJECT_ROOTS, new ModuleRootListener() {
            @Override public void rootsChanged(ModuleRootEvent event) {
                if (!applying) refresh(false);
            }
        });
    }

    public List<VirtualFile> libraryRoots() { return state.libraries(); }

    List<VirtualFile> scope(Path file) {
        State current = state;
        if (current.metadata() == null) return List.of();
        return current.metadata().scope(file.toAbsolutePath().normalize()).stream()
                .map(current.files()::get).filter(root -> root != null && root.isValid()).toList();
    }

    public synchronized void refresh(boolean report) {
        if (closed || project.isDisposed() || project.getBasePath() == null) return;
        alarm.cancelAllRequests();
        alarm.addRequest(() -> load(report), 750);
    }

    private void load(boolean report) {
        if (closed || project.isDisposed()) return;
        if (!running.compareAndSet(false, true)) {
            again.set(true);
            return;
        }
        try {
            Path root = Path.of(project.getBasePath());
            if (!Files.isRegularFile(root.resolve("Cargo.toml"))) return;
            GeneralCommandLine command = new GeneralCommandLine(cargo())
                    .withParameters("metadata", "--format-version", "1", "--locked", "--offline")
                    .withWorkDirectory(root.toFile()).withCharset(StandardCharsets.UTF_8);
            CapturingProcessHandler handler = new CapturingProcessHandler(command);
            process = handler;
            var output = handler.runProcess(60_000);
            if (closed || project.isDisposed()) return;
            if (output.isTimeout() || output.getExitCode() != 0) {
                throw new IllegalStateException("Cargo metadata is unavailable. Reload the Cargo project to download dependencies and update Cargo.lock, then use Tools > Refresh Cargo Smithy Models. " + output.getStderr());
            }
            CargoMetadata metadata = CargoMetadata.parse(output.getStdout());
            Set<Path> paths = new LinkedHashSet<>(metadata.roots());
            paths.addAll(metadata.projectSources());
            for (var pkg : metadata.packages.values()) {
                paths.addAll(pkg.sources());
                paths.addAll(pkg.models());
            }
            Map<Path, VirtualFile> files = new LinkedHashMap<>();
            for (Path path : paths) {
                VirtualFile file = LocalFileSystem.getInstance().refreshAndFindFileByNioFile(path);
                if (file != null) files.put(path, file);
            }
            List<VirtualFile> libraries = metadata.roots().stream().map(files::get)
                    .filter(file -> file != null).map(file -> file.isDirectory() ? file : file.getParent()).distinct().toList();
            State next = new State(metadata, Map.copyOf(files), libraries);
            ApplicationManager.getApplication().invokeLater(() -> apply(next, report));
        } catch (Exception error) {
            LOG.info("Unable to load Cargo Smithy models", error);
            if (report && !closed) ApplicationManager.getApplication().invokeLater(() -> notify(error.getMessage(), NotificationType.WARNING));
        } finally {
            process = null;
            running.set(false);
            if (again.getAndSet(false)) refresh(false);
        }
    }

    private static String cargo() {
        String configured = System.getenv("CARGO");
        if (configured != null && !configured.isBlank()) return configured;
        String home = System.getenv("CARGO_HOME");
        Path bin = home == null ? Path.of(System.getProperty("user.home"), ".cargo", "bin") : Path.of(home, "bin");
        Path executable = bin.resolve(System.getProperty("os.name").startsWith("Windows") ? "cargo.exe" : "cargo");
        return Files.isRegularFile(executable) ? executable.toString() : "cargo";
    }

    private void apply(State next, boolean report) {
        if (closed || project.isDisposed()) return;
        State previous = state;
        applying = true;
        try {
            ApplicationManager.getApplication().runWriteAction(() -> {
                PropertiesComponent properties = PropertiesComponent.getInstance(project);
                String[] saved = properties.getValues(SOURCE_KEY);
                Set<String> owned = new LinkedHashSet<>(saved == null ? List.of() : List.of(saved));
                Set<String> wanted = new LinkedHashSet<>();
                for (Path path : next.metadata().projectSources()) {
                    VirtualFile file = next.files().get(path);
                    if (file != null && file.isDirectory()) wanted.add(file.getUrl());
                }
                for (var module : ModuleManager.getInstance(project).getModules()) {
                    ModuleRootModificationUtil.modifyModel(module, model -> {
                        boolean changed = false;
                        for (var content : model.getContentEntries()) {
                            for (var folder : content.getSourceFolders()) {
                                if (owned.contains(folder.getUrl()) && !wanted.contains(folder.getUrl())) {
                                    content.removeSourceFolder(folder);
                                    changed = true;
                                }
                            }
                            VirtualFile root = content.getFile();
                            if (root == null) continue;
                            for (String url : wanted) {
                                VirtualFile file = VirtualFileManager.getInstance().findFileByUrl(url);
                                if (file == null || !file.toNioPath().startsWith(root.toNioPath())) continue;
                                boolean exists = List.of(content.getSourceFolders()).stream().anyMatch(folder -> folder.getUrl().equals(url));
                                if (!exists) {
                                    content.addSourceFolder(file, false);
                                    owned.add(url);
                                    changed = true;
                                }
                            }
                        }
                        return changed;
                    });
                }
                owned.retainAll(wanted);
                properties.setValues(SOURCE_KEY, owned.toArray(String[]::new));
                state = next;
                if (!previous.libraries().equals(next.libraries())) {
                    AdditionalLibraryRootsListener.fireAdditionalLibraryChanged(project, "Cargo Smithy models", previous.libraries(), next.libraries(), "com.leyline.smithy.cargo");
                }
                if (!next.metadata().sameGraph(previous.metadata())) {
                    PsiManager.getInstance(project).dropPsiCaches();
                    DaemonCodeAnalyzer.getInstance(project).restart();
                }
            });
            if (report) notify("Indexed " + next.libraries().size() + " Cargo Smithy model roots.", NotificationType.INFORMATION);
        } finally {
            applying = false;
        }
    }

    private void notify(String message, NotificationType type) {
        if (!project.isDisposed()) NotificationGroupManager.getInstance().getNotificationGroup("Smithy Cargo")
                .createNotification(message == null ? "Cargo Smithy indexing failed" : message, type).notify(project);
    }

    @Override public void dispose() {
        closed = true;
        alarm.cancelAllRequests();
        CapturingProcessHandler handler = process;
        if (handler != null) handler.destroyProcess();
    }
}
