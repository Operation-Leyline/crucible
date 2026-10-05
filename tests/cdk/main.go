package main

import (
    "encoding/json"
    "os"
    "fmt"

    "example.com/weather-cdk/weathercdk"
    "github.com/aws/aws-cdk-go/awscdk/v2"
    "github.com/aws/aws-cdk-go/awscdk/v2/assertions"
    "github.com/aws/jsii-runtime-go"
)

func main() {
    defer jsii.Close()
    app := awscdk.NewApp(nil)
    stack := awscdk.NewStack(app, jsii.String("Qualification"), nil)
    name := awscdk.NewCfnParameter(stack, jsii.String("Name"), &awscdk.CfnParameterProps{Type: jsii.String("String"), Default: "Primary"})
    city := weathercdk.NewCfnCity(stack, jsii.String("City"), &weathercdk.CfnCityProps{
        ServiceToken: jsii.String("arn:aws:lambda:us-east-1:123456789012:function:provider"),
        SecretArn: jsii.String("arn:aws:secretsmanager:us-east-1:123456789012:secret:weather"),
        Name: name.ValueAsString(), Mode: jsii.String("fast"),
        Settings: &weathercdk.SettingsProperty{Enabled: true, Replicas: jsii.Number(2)},
        Tags: &[]*string{jsii.String("a"), jsii.String("b")}, Labels: &map[string]*string{"Team": jsii.String("platform")},
    })
    city.ApplyRemovalPolicy(awscdk.RemovalPolicy_RETAIN, nil)
    dependent := weathercdk.NewCfnCity(stack, jsii.String("Dependent"), &weathercdk.CfnCityProps{
        ServiceToken: jsii.String("arn:aws:lambda:us-east-1:123456789012:function:provider"),
        SecretArn: jsii.String("arn:aws:secretsmanager:us-east-1:123456789012:secret:weather"), Name: jsii.String("Secondary"),
    })
    dependent.AddResourceDependency(city, nil)
    awscdk.NewCfnOutput(stack, jsii.String("CityId"), &awscdk.CfnOutputProps{Value: city.AttrId()})
    data, err := json.Marshal(assertions.Template_FromStack(stack, nil).ToJSON())
    if err != nil { panic(err) }
    if err = os.WriteFile("template.json", data, 0600); err != nil { panic(err) }
    fmt.Println("Installed Go consumer synthesized successfully")
}
