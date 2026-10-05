using Amazon.CDK;
using Amazon.CDK.Assertions;
using Example.Weather;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;

var app = new App();
var stack = new Stack(app, "Qualification");
var name = new CfnParameter(stack, "Name", new CfnParameterProps { Type = "String", Default = "Primary" });
var city = new CfnCity(stack, "City", new CfnCityProps {
    ServiceToken = "arn:aws:lambda:us-east-1:123456789012:function:provider",
    SecretArn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:weather",
    Name = name.ValueAsString, Mode = "fast", Settings = new SettingsProperty { Enabled = true, Replicas = 2 },
    Tags = new[] { "a", "b" }, Labels = new Dictionary<string, string> { ["Team"] = "platform" },
});
city.ApplyRemovalPolicy(RemovalPolicy.RETAIN);
var dependent = new CfnCity(stack, "Dependent", new CfnCityProps {
    ServiceToken = "arn:aws:lambda:us-east-1:123456789012:function:provider",
    SecretArn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:weather", Name = "Secondary",
});
dependent.AddResourceDependency(city);
new CfnOutput(stack, "CityId", new CfnOutputProps { Value = city.AttrId });
File.WriteAllText("template.json", JsonSerializer.Serialize(Template.FromStack(stack).ToJSON()));
System.Console.WriteLine("Installed .NET consumer synthesized successfully");
