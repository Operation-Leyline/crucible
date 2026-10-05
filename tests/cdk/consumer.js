const assert = require('node:assert/strict');
const { Stack } = require('aws-cdk-lib');
const { Template } = require('aws-cdk-lib/assertions');
const { CfnCity } = require('@example/weather-cdk');
const stack = new Stack();
new CfnCity(stack, 'City', { serviceToken: 'provider', secretArn: 'secret', name: 'JavaScript' });
assert.equal(Template.fromStack(stack).toJSON().Resources.City.Type, 'Custom::WeatherCity');
console.log('Installed JavaScript consumer passed');
