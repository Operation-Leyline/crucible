#include "weather_client/models.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
int main(void) {
    weather_arena *arena = weather_arena_new();
    weather_error error = {0};
    weather_models_root root = {0};
    weather_models_node node = {0};
    weather_models_loop empty = {0};
    const weather_models_loop *items[] = {&empty};
    weather_models_loop loop = {1, items};
    weather_models_choice choice = {0};
    const weather_models_root *copy;
    const weather_json *wire;
    weather_string text;
    root.node.state = weather_present; root.node.value = &node;
    root.loop.state = weather_present; root.loop.value = &loop;
    root.choice.state = weather_present; root.choice.value = &choice;
    root.document.state = weather_null;
    choice.tag = weather_models_choice_tag_choice; choice.state = weather_null;
    assert(weather_models_root_encode(arena, &root, &wire, &error) == weather_ok);
    assert(weather_models_root_decode(arena, wire, &copy, &error) == weather_ok);
    assert(copy->loop.value->length == 1 && copy->loop.value->items[0]->length == 0);
    assert(copy->choice.value->tag == weather_models_choice_tag_choice && copy->choice.value->state == weather_null);
    assert(copy->document.state == weather_null);
    assert(weather_models_root_decode(arena, wire, NULL, &error) == weather_invalid);
    assert(error.status == weather_invalid);
    assert(weather_json_write(arena, wire, &text, &error) == weather_ok);
    assert(strstr(text.data, "\"loop\":[[]]"));
    node.next.state = weather_present; node.next.value = &node;
    assert(weather_models_root_encode(arena, &root, &wire, &error) == weather_invalid);
    weather_arena_free(arena);
    puts("C recursive structures, list-only recursion and nullable union passed");
    return 0;
}
