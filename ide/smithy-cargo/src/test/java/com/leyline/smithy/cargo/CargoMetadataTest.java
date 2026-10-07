package com.leyline.smithy.cargo;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/** Standalone assertions keep graph discovery testable without launching an IDE. */
public final class CargoMetadataTest {
    private static JsonObject pkg(String id, Path root, String sources, String models) {
        JsonObject pkg = new JsonObject();
        pkg.addProperty("id", id);
        pkg.addProperty("manifest_path", root.resolve("Cargo.toml").toString());
        JsonObject metadata = new JsonObject();
        JsonObject smithy = new JsonObject();
        if (sources != null) {
            JsonArray array = new JsonArray(); array.add(sources); smithy.add("sources", array);
        }
        if (models != null) {
            JsonArray array = new JsonArray(); array.add(models); smithy.add("models", array);
        }
        metadata.add("smithy", smithy); pkg.add("metadata", metadata);
        return pkg;
    }

    private static JsonObject node(String id, String... dependencies) {
        JsonObject node = new JsonObject(); node.addProperty("id", id);
        JsonArray deps = new JsonArray();
        for (String dep : dependencies) {
            JsonObject edge = new JsonObject(); edge.addProperty("pkg", dep);
            // Real build-dependency edges must be traversed too.
            JsonObject kind = new JsonObject(); kind.addProperty("kind", "build");
            JsonArray kinds = new JsonArray(); kinds.add(kind); edge.add("dep_kinds", kinds);
            deps.add(edge);
        }
        node.add("deps", deps); return node;
    }

    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("smithy-cargo-test-");
        Path first = root.resolve("first"); Path second = root.resolve("second");
        Path shared = root.resolve("traits"); Path old = root.resolve("smithy-1.73");
        Path current = root.resolve("smithy-1.74"); Path unused = root.resolve("unused");
        JsonObject metadata = new JsonObject(); JsonArray packages = new JsonArray();
        packages.add(pkg("first", first, "smithy", null));
        packages.add(pkg("second", second, "smithy", null));
        packages.add(pkg("traits", shared, null, "crucible.smithy"));
        packages.add(pkg("old", old, null, "src/prelude.smithy"));
        packages.add(pkg("current", current, null, "src/prelude.smithy"));
        packages.add(pkg("unused", unused, null, "src/prelude.smithy"));
        metadata.add("packages", packages);
        JsonArray workspace = new JsonArray(); workspace.add("first"); workspace.add("second");
        metadata.add("workspace_members", workspace);
        JsonObject resolve = new JsonObject(); JsonArray nodes = new JsonArray();
        nodes.add(node("first", "old", "traits")); nodes.add(node("second", "current", "traits"));
        nodes.add(node("old")); nodes.add(node("current")); nodes.add(node("traits"));
        resolve.add("nodes", nodes); metadata.add("resolve", resolve);
        CargoMetadata graph = CargoMetadata.parse(metadata.toString());
        List<Path> firstScope = graph.scope(first.resolve("smithy/hello.smithy"));
        check(firstScope.contains(old.resolve("src/prelude.smithy")), "missing old prelude");
        check(!firstScope.contains(current.resolve("src/prelude.smithy")), "mixed prelude versions");
        check(graph.scope(second.resolve("smithy/hello.smithy")).contains(current.resolve("src/prelude.smithy")), "missing current prelude");
        check(graph.roots().contains(shared.resolve("crucible.smithy")), "missing transitive traits");
        check(!graph.roots().contains(unused.resolve("src/prelude.smithy")), "indexed disabled optional dependency");
        check(graph.scope(shared.resolve("crucible.smithy")).contains(old.resolve("src/prelude.smithy")), "custom traits cannot resolve their implicit prelude");
        check(graph.projectSources().equals(List.of(first.resolve("smithy"), second.resolve("smithy")))
                || graph.projectSources().equals(List.of(second.resolve("smithy"), first.resolve("smithy"))), "wrong project source roots");
        packages.add(pkg("nested", first.resolve("nested"), "smithy", null));
        graph = CargoMetadata.parse(metadata.toString());
        check(graph.owner(first.resolve("nested/smithy/api.smithy")).id().equals("nested"), "workspace package ownership was ambiguous");
        packages.add(pkg("escape", root.resolve("escape"), null, "../outside.smithy"));
        try {
            CargoMetadata.parse(metadata.toString());
            throw new AssertionError("accepted a model path outside its dependency");
        } catch (IllegalArgumentException expected) {
            check(expected.getMessage().contains("inside their Cargo package"), "wrong unsafe-path diagnostic");
        }
        Files.delete(root);
        System.out.println("Cargo graph, build dependencies, version isolation, package ownership and metadata path tests passed");
    }

    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
}
