package com.leyline.smithy.cargo;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** Cargo's resolved graph is authoritative, including build dependencies and features. */
final class CargoMetadata {
    record ModelPackage(String id, Path directory, List<Path> sources, List<Path> models) {}
    final Map<String, ModelPackage> packages;
    final Map<String, List<String>> dependencies;
    final Set<String> workspace;

    private CargoMetadata(Map<String, ModelPackage> packages, Map<String, List<String>> dependencies, Set<String> workspace) {
        this.packages = Map.copyOf(packages);
        this.dependencies = Map.copyOf(dependencies);
        this.workspace = Set.copyOf(workspace);
    }

    static CargoMetadata parse(String json) {
        JsonObject metadata = JsonParser.parseString(json).getAsJsonObject();
        Map<String, ModelPackage> packages = new LinkedHashMap<>();
        for (JsonElement item : metadata.getAsJsonArray("packages")) {
            JsonObject pkg = item.getAsJsonObject();
            String id = pkg.get("id").getAsString();
            Path directory = Path.of(pkg.get("manifest_path").getAsString()).toAbsolutePath().normalize().getParent();
            JsonObject smithy = new JsonObject();
            JsonElement settings = pkg.get("metadata");
            if (settings != null && settings.isJsonObject() && settings.getAsJsonObject().has("smithy")) {
                smithy = settings.getAsJsonObject().getAsJsonObject("smithy");
            }
            List<Path> sources = paths(directory, smithy, "sources");
            // Existing projects using the conventional directory need no manifest edit.
            if (sources.isEmpty() && Files.isDirectory(directory.resolve("smithy"))) {
                sources = List.of(directory.resolve("smithy"));
            }
            packages.put(id, new ModelPackage(id, directory, sources, paths(directory, smithy, "models")));
        }
        Map<String, List<String>> dependencies = new LinkedHashMap<>();
        JsonElement resolve = metadata.get("resolve");
        if (resolve == null || !resolve.isJsonObject()) {
            throw new IllegalArgumentException("full cargo metadata is required; do not use --no-deps");
        }
        for (JsonElement item : resolve.getAsJsonObject().getAsJsonArray("nodes")) {
            JsonObject node = item.getAsJsonObject();
            List<String> deps = new ArrayList<>();
            for (JsonElement dep : node.getAsJsonArray("deps")) {
                deps.add(dep.getAsJsonObject().get("pkg").getAsString());
            }
            dependencies.put(node.get("id").getAsString(), List.copyOf(deps));
        }
        Set<String> workspace = new LinkedHashSet<>();
        for (JsonElement id : metadata.getAsJsonArray("workspace_members")) workspace.add(id.getAsString());
        return new CargoMetadata(packages, dependencies, workspace);
    }

    private static List<Path> paths(Path directory, JsonObject smithy, String field) {
        if (!smithy.has(field)) return List.of();
        List<Path> result = new ArrayList<>();
        for (JsonElement entry : smithy.getAsJsonArray(field)) {
            Path relative = Path.of(entry.getAsString());
            Path path = directory.resolve(relative).normalize();
            if (relative.isAbsolute() || !path.startsWith(directory)) {
                throw new IllegalArgumentException("Smithy metadata paths must stay inside their Cargo package: " + entry.getAsString());
            }
            result.add(path);
        }
        return List.copyOf(result);
    }

    Set<String> reachable(String id) {
        Set<String> result = new LinkedHashSet<>();
        ArrayDeque<String> pending = new ArrayDeque<>();
        pending.add(id);
        while (!pending.isEmpty()) {
            String next = pending.removeFirst();
            if (result.add(next)) pending.addAll(dependencies.getOrDefault(next, List.of()));
        }
        return result;
    }

    ModelPackage owner(Path file) {
        return packages.values().stream().filter(pkg -> file.startsWith(pkg.directory()))
                .max(Comparator.comparingInt(pkg -> pkg.directory().getNameCount())).orElse(null);
    }

    List<Path> scope(Path file) {
        ModelPackage owner = owner(file);
        if (owner == null) return List.of();
        Set<Path> roots = new LinkedHashSet<>();
        for (String id : reachable(owner.id())) {
            ModelPackage pkg = packages.get(id);
            if (pkg != null) {
                roots.addAll(pkg.sources());
                roots.addAll(pkg.models());
            }
        }
        // Smithy supplies its prelude implicitly, whereas a definitions-only
        // Cargo package need not depend on a compiler. Its annotations still
        // need the consuming workspace's prelude to classify custom traits.
        if (!workspace.contains(owner.id())) {
            for (String member : workspace) {
                Set<String> closure = reachable(member);
                if (!closure.contains(owner.id())) continue;
                for (String id : closure) {
                    ModelPackage pkg = packages.get(id);
                    if (pkg != null) {
                        for (Path model : pkg.models()) {
                            if (model.getFileName().toString().equals("prelude.smithy")) roots.add(model);
                        }
                    }
                }
            }
        }
        return List.copyOf(roots);
    }

    boolean sameGraph(CargoMetadata other) {
        return other != null && packages.equals(other.packages) && dependencies.equals(other.dependencies) && workspace.equals(other.workspace);
    }

    List<Path> roots() {
        Set<Path> roots = new LinkedHashSet<>();
        for (String member : workspace) {
            for (String id : reachable(member)) {
                ModelPackage pkg = packages.get(id);
                if (pkg != null) {
                    roots.addAll(pkg.models());
                    if (!workspace.contains(id)) roots.addAll(pkg.sources());
                }
            }
        }
        return List.copyOf(roots);
    }

    List<Path> projectSources() {
        return workspace.stream().map(packages::get).filter(pkg -> pkg != null)
                .flatMap(pkg -> pkg.sources().stream()).distinct().toList();
    }
}
