#include "wire_client/client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(bool value) { if (!value) { fputs("Wire check failed\n", stderr); exit(1); } }
static void header(const wire_request *request, const char *name, const char *expected) {
    wire_string value;
    check(wire_header_get(request->headers, wire_string_view(name), &value));
    check(wire_string_equal(value, wire_string_view(expected)));
}
static wire_status send_fake(void *user, const wire_request *request, bool read_body,
    const wire_scope *scope, wire_arena *arena, wire_response *response, wire_error *error) {
    (void)scope; check(read_body); check(request->has_body);
    if (wire_string_equal(request->url, wire_string_view("http://127.0.0.1/document"))) {
        header(request, "content-type", "application/json");
        check(request->body.length == strlen("{\"name\":\"record\"}") && memcmp(request->body.data, "{\"name\":\"record\"}", request->body.length) == 0);
        response->status = 200; response->body = request->body; return wire_ok;
    }
    header(request, "content-type", "text/plain");
    header(request, "x-meta", "eyJhIjoxfQ==, aGVsbG8=");
    check(request->body.length == 6 && memcmp(request->body.data, "h\xc3\xa9llo", 6) == 0);
    bool failure = *(bool *)user;
    response->status = failure ? 422 : 200;
    if (failure) {
        response->body = (wire_blob){(const unsigned char *)"invalid text", 12};
        return wire_header_set(arena, &response->headers, wire_string_view("x-crucible-error"), wire_string_view("TextError"), error);
    }
    response->body = request->body;
    wire_status status = wire_header_set(arena, &response->headers, wire_string_view("x-meta"), wire_string_view("eyJhIjoxfQ==, aGVsbG8="), error);
    return status;
}
int main(void) {
    bool failure = false;
    wire_client_config config = wire_client_defaults(); config.endpoint = wire_string_view("http://127.0.0.1"); config.send = send_fake; config.transport_user = &failure;
    wire_client *client = NULL; wire_error error = {0}; check(wire_client_new(&config, &client, &error) == wire_ok);
    wire_string media[] = {wire_string_view("{\"a\":1}"), wire_string_view("hello")}; wire_models_media_list list = {2, media};
    wire_models_text_input input = {0}; wire_models_text_input_set_text(&input, wire_string_view("h\xc3\xa9llo")); wire_models_text_input_set_meta(&input, &list);
    wire_operations_text_result result = {0}; check(wire_operations_text(client, &input, NULL, NULL, &result, &error) == wire_ok);
    check(wire_string_equal(result.value->text.value, input.text.value)); check(result.value->meta.value->length == 2);
    wire_operations_text_result_free(&result);
    wire_models_record record = {0}; wire_models_record_set_name(&record, wire_string_view("record"));
    wire_models_document_input document = {0}; wire_models_document_input_set_document(&document, &record);
    wire_operations_document_result doc_result = {0}; check(wire_operations_document(client, &document, NULL, NULL, &doc_result, &error) == wire_ok);
    check(wire_string_equal(doc_result.value->document.value->name.value, wire_string_view("record"))); wire_operations_document_result_free(&doc_result);
    failure = true; check(wire_operations_text(client, &input, NULL, NULL, &result, &error) == wire_service_error);
    const wire_models_text_error *modeled = wire_error_as_text_error(&result.base);
    check(modeled && wire_string_equal(modeled->details.value, wire_string_view("invalid text")) && result.base.response.status == 422);
    wire_operations_text_result_free(&result); wire_client_free(client);
    puts("C payloads and media headers passed"); return 0;
}
