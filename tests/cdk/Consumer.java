import com.example.weather.CfnCity;
import com.example.weather.CfnCityProps;
import com.example.weather.SettingsProperty;
import software.amazon.awscdk.App;
import software.amazon.awscdk.Stack;
import software.amazon.awscdk.CfnParameter;
import software.amazon.awscdk.CfnOutput;
import software.amazon.awscdk.RemovalPolicy;
import software.amazon.awscdk.assertions.Template;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

public class Consumer {
    public static void main(String[] args) throws Exception {
        App app = new App();
        Stack stack = new Stack(app, "Qualification");
        CfnParameter name = CfnParameter.Builder.create(stack, "Name").type("String").defaultValue("Primary").build();
        CfnCity city = new CfnCity(stack, "City", CfnCityProps.builder()
            .serviceToken("arn:aws:lambda:us-east-1:123456789012:function:provider")
            .secretArn("arn:aws:secretsmanager:us-east-1:123456789012:secret:weather")
            .name(name.getValueAsString()).mode("fast")
            .settings(SettingsProperty.builder().enabled(true).replicas(2).build())
            .tags(List.of("a", "b")).labels(Map.of("Team", "platform")).build());
        city.applyRemovalPolicy(RemovalPolicy.RETAIN);
        CfnCity dependent = new CfnCity(stack, "Dependent", CfnCityProps.builder()
            .serviceToken("arn:aws:lambda:us-east-1:123456789012:function:provider")
            .secretArn("arn:aws:secretsmanager:us-east-1:123456789012:secret:weather").name("Secondary").build());
        dependent.addResourceDependency(city);
        CfnOutput.Builder.create(stack, "CityId").value(city.getAttrId()).build();
        Files.writeString(Path.of("template.json"), new ObjectMapper().writeValueAsString(Template.fromStack(stack).toJSON()));
        System.out.println("Installed Java consumer synthesized successfully");
    }
}
