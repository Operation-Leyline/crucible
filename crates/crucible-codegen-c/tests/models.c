#include "weather_client/models.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void good(weather_status status, const weather_error *error) {
    if (status != weather_ok) { fprintf(stderr, "model failure: %s\n", error->message); assert(status == weather_ok); }
}
static const weather_json *parse(weather_arena *arena, const char *text, weather_error *error) {
    const weather_json *json = NULL;
    good(weather_json_parse(arena, weather_string_view(text), &json, error), error);
    return json;
}
int main(void) {
    weather_arena *source = weather_arena_new(), *decoded = weather_arena_new(), *encoded = weather_arena_new();
    weather_error error = {0};
    const weather_models_city_summary *city = NULL;
    const weather_json *json, *wire;
    weather_string text;
    weather_models_create_city_input input = {0};
    weather_models_location location = {0};
    weather_models_photo_summary photo = {0};
    const weather_models_photo_summary *photo_copy;
    const unsigned char bytes[] = {0, 128, 255};
    assert(source && decoded && encoded);
    json = parse(source, "{\"id\":\"x\",\"name\":\"city\",\"population\":9223372036854775807,\"createdAt\":-0.000000001}", &error);
    good(weather_models_city_summary_decode(decoded, json, &city, &error), &error);
    weather_arena_free(source); source = weather_arena_new();
    assert(weather_models_city_summary_has_population(city) && weather_models_city_summary_get_population(city) == INT64_MAX);
    assert(weather_string_equal(city->name.value, weather_string_view("city")));
    assert(city->created_at.value.seconds == -1 && city->created_at.value.nanoseconds == 999999999);
    good(weather_models_city_summary_encode(encoded, city, &wire, &error), &error);
    good(weather_json_write(encoded, wire, &text, &error), &error);
    assert(strstr(text.data, "9223372036854775807"));
    json = parse(source, "{\"id\":\"x\",\"name\":\"default\"}", &error);
    good(weather_models_city_summary_decode(decoded, json, &city, &error), &error);
    assert(city->population.state == weather_present && city->population.value == 0);
    json = parse(source, "{\"id\":\"x\",\"name\":\"bad\",\"population\":null}", &error);
    assert(weather_models_city_summary_decode(decoded, json, &city, &error) == weather_invalid);
    location.tag = weather_models_location_unknown;
    location.state = weather_present;
    location.unknown_name = weather_string_view("future");
    location.unknown_value = parse(source, "{\"n\":9223372036854775807,\"decimal\":1.000000000000000000001}", &error);
    weather_models_create_city_input_set_name(&input, weather_string_view("ordinary"));
    assert(weather_models_create_city_input_has_name(&input));
    assert(weather_string_equal(weather_models_create_city_input_get_name(&input), weather_string_view("ordinary")));
    weather_models_create_city_input_clear_name(&input);
    assert(!weather_models_create_city_input_has_name(&input));
    assert(weather_models_create_city_input_get_name(&input).length == 0);
    weather_models_create_city_input_null_name(&input);
    input.location.state = weather_present; input.location.value = &location;
    good(weather_models_create_city_input_encode(encoded, &input, &wire, &error), &error);
    assert(weather_json_type(weather_json_get(wire, weather_string_view("name"))) == weather_json_null);
    assert(!weather_json_get(wire, weather_string_view("clientToken")));
    { const weather_models_create_city_input *copy;
      good(weather_models_create_city_input_decode(decoded, wire, &copy, &error), &error);
      assert(copy->location.value->tag == weather_models_location_unknown);
      good(weather_json_write(decoded, copy->location.value->unknown_value, &text, &error), &error);
      assert(strstr(text.data, "1.000000000000000000001")); }
    location.unknown_name = weather_string_view("coordinates");
    assert(weather_models_create_city_input_encode(encoded, &input, &wire, &error) == weather_invalid);
    photo.size.state = weather_present; photo.size.value = 3;
    photo.thumbnail.state = weather_present; photo.thumbnail.value.data = bytes; photo.thumbnail.value.length = sizeof(bytes);
    good(weather_models_photo_summary_encode(encoded, &photo, &wire, &error), &error);
    good(weather_models_photo_summary_decode(decoded, wire, &photo_copy, &error), &error);
    assert(photo_copy->thumbnail.value.length == 3 && memcmp(photo_copy->thumbnail.value.data, bytes, 3) == 0);
    { weather_models_forecast_entry entries[1]; weather_models_forecast forecast;
      const weather_models_forecast *copy;
      entries[0].key = weather_models_sky_known_clear(); entries[0].value = parse(source, "null", &error);
      forecast.length = 1; forecast.items = entries;
      good(weather_models_forecast_encode(encoded, &forecast, &wire, &error), &error);
      good(weather_models_forecast_decode(decoded, wire, &copy, &error), &error);
      assert(copy->length == 1 && weather_string_equal(copy->items[0].key, entries[0].key));
      assert(weather_json_type(copy->items[0].value) == weather_json_null); }
    { const weather_shape *type = weather_shape_lookup(weather_string_view("smithy.api#Long"));
      const void *value; int64_t number = INT64_MIN;
      assert(type);
      good(weather_model_encode(encoded, type, &number, &wire, &error), &error);
      good(weather_model_decode(decoded, type, wire, &value, &error), &error);
      assert(*(const int64_t *)value == INT64_MIN); }
    weather_arena_free(source); weather_arena_free(encoded); weather_arena_free(decoded);
    puts("C weather native models, presence, precision, blobs, enums and arena ownership passed");
    return 0;
}
