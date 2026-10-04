<?php
declare(strict_types=1);
spl_autoload_register(function (string $class): void {
    $path = (getenv('PHP_PACKAGE') ?: __DIR__) . '/src/' . str_replace('\\', '/', substr($class, strlen('Example\\Weather\\'))) . '.php';
    require $path;
});
use Example\Weather\Models\{CitySummary, CreateCityInput, LocationUnknown, LocationUnknownVariant};
use Example\Weather\Runtime\{Field, Timestamp, ExactJson, JsonValue, Unit};
use Example\Weather\Schema;
function check(bool $value, string $message): void { if (!$value) throw new RuntimeException($message); }
$codec = Schema::codec();
$context = $codec->context('example.weather#CitySummary');
$model = new CitySummary(id:'x', name:'name', population:PHP_INT_MAX, updatedAt:Field::value(Timestamp::parse('2026-10-04T00:00:00.123456789Z')));
$copy = $codec->decode(ExactJson::decode(ExactJson::encode($codec->encode($model, $context))), $context);
check($copy->population === PHP_INT_MAX, 'Long precision');
check($copy->updatedAt->value->nanoseconds === 123456789, 'Timestamp precision');
$default = $codec->decode(ExactJson::decode('{"id":"x","name":"n"}'),$context);
check($default->population === 0, 'Default integer');
$inputContext = $codec->context('example.weather#CreateCityInput');
$input = new CreateCityInput(name:Field::value('n'), location:Field::nullValue());
$encoded = $codec->encode($input,$inputContext);
check($encoded->has('location') && $encoded->get('location') === null && !$encoded->has('clientToken'), 'Absent/null');
$union = $codec->context('example.weather#Location');
foreach ([new LocationUnknown(new Unit()),new LocationUnknownVariant('future',JsonValue::parse('{"n":9223372036854775807}'))] as $value) {
    $wire = $codec->encode($value,$union);
    check(ExactJson::encode($codec->encode($codec->decode($wire,$union),$union)) === ExactJson::encode($wire),'Union round trip');
}
echo "PHP generated model round trips passed\n";
