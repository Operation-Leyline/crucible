/* Payload Content-Type precedence: a modelled Content-Type member, then the
 * payload's media type, then a client-wide default header. test-c.py builds this
 * once per protocol, replacing the payload_ prefix. */
#include "payload_client/client.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(bool value, const char *message) { if (!value) { fprintf(stderr, "Payload Content-Type check failed: %s\n", message); exit(1); } }
static bool content_type(payload_string name) {
    static const char expected[] = "content-type";
    if (name.length != sizeof(expected) - 1) return false;
    for (size_t i = 0; i < name.length; ++i) {
        char c = name.data[i]; if (c >= 'A' && c <= 'Z') c = (char)(c + 32);
        if (c != expected[i]) return false;
    }
    return true;
}
static payload_status send_fake(void *user, const payload_request *request, bool read_body,
    const payload_scope *scope, payload_arena *arena, payload_response *response, payload_error *error) {
    (void)read_body; (void)scope; (void)arena; (void)error;
    const char *expected = user;
    check(request->has_body && request->body.length == 3 && memcmp(request->body.data, "PNG", 3) == 0, "body");
    size_t seen = 0; payload_string value = {0};
    for (size_t i = 0; i < request->headers.count; ++i)
        if (content_type(request->headers.items[i].name)) { ++seen; value = request->headers.items[i].value; }
    check(seen == 1, "exactly one Content-Type header");
    check(payload_string_equal(value, payload_string_view(expected)), expected);
    response->status = 200; return payload_ok;
}
int main(void) {
    const char *members[] = {NULL, "image/png"}, *defaults[] = {NULL, "image/jpeg"};
    for (size_t m = 0; m < 2; ++m) for (size_t d = 0; d < 2; ++d) {
        payload_header header = {payload_string_view("Content-Type"), payload_string_view("image/jpeg")};
        payload_client_config config = payload_client_defaults();
        config.endpoint = payload_string_view("http://127.0.0.1"); config.send = send_fake;
        config.transport_user = (void *)(members[m] ? members[m] : "application/octet-stream");
        if (defaults[d]) config.headers = (payload_headers){&header, 1};
        payload_client *client = NULL; payload_error error = {0};
        check(payload_client_new(&config, &client, &error) == payload_ok, "client");
        payload_models_upload_input input = {0};
        payload_models_upload_input_set_data(&input, (payload_blob){(const unsigned char *)"PNG", 3});
        if (members[m]) payload_models_upload_input_set_content_type(&input, payload_string_view(members[m]));
        payload_operations_upload_result result = {0};
        check(payload_operations_upload(client, &input, NULL, NULL, &result, &error) == payload_ok, "upload");
        payload_operations_upload_result_free(&result); payload_client_free(client);
    }
    puts("C payload Content-Type precedence passed"); return 0;
}
