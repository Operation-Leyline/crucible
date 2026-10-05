import assert = require('node:assert/strict');
import fs = require('node:fs');
import { App, CfnOutput, CfnParameter, Fn, RemovalPolicy, Stack } from 'aws-cdk-lib';
import { Template } from 'aws-cdk-lib/assertions';
import { CfnCity, CfnCityProps, SettingsProperty } from '@example/weather-cdk';

const app = new App();
const stack = new Stack(app, 'Qualification');
const name = new CfnParameter(stack, 'Name', { type: 'String', default: 'Primary' });
const settings: SettingsProperty = { enabled: true, replicas: 2 };
const city = new CfnCity(stack, 'City', {
  serviceToken: 'arn:aws:lambda:us-east-1:123456789012:function:provider',
  secretArn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:weather',
  name: name.valueAsString, mode: 'fast', settings, tags: ['a', 'b'], labels: { Team: 'platform' },
});
city.applyRemovalPolicy(RemovalPolicy.RETAIN);
const dependent = new CfnCity(stack, 'Dependent', {
  serviceToken: 'arn:aws:lambda:us-east-1:123456789012:function:provider',
  secretArn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:weather',
  name: 'Secondary',
});
dependent.addResourceDependency(city);
new CfnOutput(stack, 'CityId', { value: city.attrId });
const template = Template.fromStack(stack).toJSON();
assert.equal(template.Resources.City.Type, 'Custom::WeatherCity');
assert.equal(template.Resources.Dependent.Type, 'Custom::WeatherCity');
assert.deepEqual(template.Resources.City.Properties, {
  ServiceToken: 'arn:aws:lambda:us-east-1:123456789012:function:provider',
  SecretArn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:weather',
  Name: { Ref: 'Name' }, Mode: 'fast', Settings: { enabled: true, replicas: 2 },
  Tags: ['a', 'b'], Labels: { Team: 'platform' },
});
assert.deepEqual(template.Resources.Dependent.Properties, {
  ServiceToken: 'arn:aws:lambda:us-east-1:123456789012:function:provider',
  SecretArn: 'arn:aws:secretsmanager:us-east-1:123456789012:secret:weather', Name: 'Secondary',
});
assert.equal(template.Resources.City.DeletionPolicy, 'Retain');
assert.equal(template.Resources.City.UpdateReplacePolicy, 'Retain');
assert.deepEqual(template.Resources.Dependent.DependsOn, ['City']);
assert.deepEqual(template.Outputs.CityId.Value, { 'Fn::GetAtt': ['City', 'Id'] });
assert.equal(Object.keys(template.Resources).length, 2);
fs.writeFileSync('template.json', JSON.stringify(template));

const valid: CfnCityProps = { serviceToken: 'provider', secretArn: 'secret', name: 'Valid' };
let index = 0;
function rejected(props: object, message: RegExp): void {
  assert.throws(() => new CfnCity(new Stack(app, `Invalid${index++}`), 'Resource', { ...valid, ...props } as CfnCityProps), message);
}
rejected({ name: '' }, /name: below minimum length/);
rejected({ name: 'A'.repeat(41) }, /name: above maximum length/);
rejected({ name: '1Invalid' }, /name: pattern mismatch/);
rejected({ settings: {} }, /settings.enabled: required/);
rejected({ settings: { enabled: true, replicas: 4 } }, /settings.replicas: above maximum/);
rejected({ settings: { enabled: true, replicas: 1.5 } }, /settings.replicas: expected safe integer/);
rejected({ tags: ['a', 'a'] }, /tags: duplicate item/);
rejected({ tags: ['a', 'b', 'c', 'd'] }, /tags: above maximum length/);
rejected({ mode: 'invalid' }, /mode: invalid enum value/);
rejected({ labels: { '1Invalid': 'value' } }, /labels.<key>: pattern mismatch/);
rejected({ name: null }, /name: null/);
rejected({ name: undefined }, /name: required/);
const tokens = new Stack(app, 'Tokens');
const tokenName = new CfnParameter(tokens, 'Name');
new CfnCity(tokens, 'Resource', {
  ...valid, name: tokenName.valueAsString,
  settings: Fn.conditionIf('Enabled', { enabled: true }, { enabled: false }),
  tags: [tokenName.valueAsString, tokenName.valueAsString],
});
console.log('Installed TypeScript consumer: tokens, nested constraints, attributes, retention, dependencies passed');
