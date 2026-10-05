import json
from pathlib import Path

from aws_cdk import App, CfnOutput, CfnParameter, RemovalPolicy, Stack
from aws_cdk.assertions import Template
from example_weather_cdk import CfnCity, SettingsProperty

app = App()
stack = Stack(app, "Qualification")
name = CfnParameter(stack, "Name", type="String", default="Primary")
city = CfnCity(stack, "City",
    service_token="arn:aws:lambda:us-east-1:123456789012:function:provider",
    secret_arn="arn:aws:secretsmanager:us-east-1:123456789012:secret:weather",
    name=name.value_as_string, mode="fast", settings=SettingsProperty(enabled=True, replicas=2),
    tags=["a", "b"], labels={"Team": "platform"})
city.apply_removal_policy(RemovalPolicy.RETAIN)
dependent = CfnCity(stack, "Dependent",
    service_token="arn:aws:lambda:us-east-1:123456789012:function:provider",
    secret_arn="arn:aws:secretsmanager:us-east-1:123456789012:secret:weather", name="Secondary")
dependent.add_resource_dependency(city)
CfnOutput(stack, "CityId", value=city.attr_id)
Path("template.json").write_text(json.dumps(Template.from_stack(stack).to_json()), encoding="utf-8")
print("Installed Python consumer synthesized successfully")
