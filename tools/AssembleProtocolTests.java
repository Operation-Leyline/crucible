import java.nio.file.Files;
import java.nio.file.Path;
import software.amazon.smithy.model.Model;
import software.amazon.smithy.model.node.Node;
import software.amazon.smithy.model.shapes.ModelSerializer;
import software.amazon.smithy.model.transform.ModelTransformer;

/** Use Smithy's own validation and mixin transform before exporting test APIs. */
public final class AssembleProtocolTests {
    public static void main(String[] args) throws Exception {
        var assembler = Model.assembler().discoverModels();
        for (int i = 1; i < args.length; i++) assembler.addImport(Path.of(args[i]));
        var result = assembler.assemble();
        result.getValidationEvents().forEach(System.err::println);
        var model = ModelTransformer.create().flattenAndRemoveMixins(result.unwrap());
        var serializer = ModelSerializer.builder().shapeFilter(shape -> {
            String namespace = shape.getId().getNamespace();
            return namespace.startsWith("aws.protocoltests.") || namespace.equals("com.amazonaws.s3");
        }).build();
        Files.writeString(Path.of(args[0]), Node.prettyPrintJson(serializer.serialize(model)) + "\n");
    }
}
