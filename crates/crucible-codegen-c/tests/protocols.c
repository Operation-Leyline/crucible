/* Compiled once per AWS JSON protocol; the runner replaces the proto prefix. */
#include "proto_client/client.h"
#include <math.h>
#define PREFIX proto
#include "echo.h"

#define VIEW(text) proto_string_view(text)
static proto_client *client_for(const char *mode) {
    static proto_header header;
    header = (proto_header){VIEW("x-mode"), VIEW(mode)};
    proto_client_config config = proto_client_defaults();
    config.endpoint = VIEW(environment("FEATURES_URL")); config.headers = (proto_headers){&header, 1};
    proto_client *client = NULL; proto_error error = {0};
    good(proto_client_new(&config, &client, &error), &error, "client_new");
    return client;
}

int main(void) {
    proto_error error = {0};
    proto_timestamp instant = {-1, 750000000};
    proto_timestamp instants[] = {instant};
    proto_models_times times = {1, instants};
    proto_models_values_input input = {0};
    proto_models_values_input_set_renamed(&input, VIEW("hello"));
    proto_models_values_input_set_when(&input, instant);
    proto_models_values_input_set_iso(&input, instant);
    proto_models_values_input_set_date(&input, instant);
    proto_models_values_input_set_dates(&input, &times);
    proto_models_values_input_set_query(&input, instant);
    proto_models_values_input_set_float_value(&input, NAN);
    proto_models_values_input_set_double_value(&input, INFINITY);
    proto_models_values_input_null_nullable(&input);
    proto_models_values_input_null_document(&input);

    proto_client *client = client_for("echo");
    proto_operations_echo_result result = {0};
    good(proto_operations_echo(client, &input, NULL, NULL, &result, &error), &error, "echo");
    const proto_models_values_output *output = result.value;
    check(output->nullable.state == proto_absent, "null structure member is omitted");
    check(output->document.state == proto_null, "document null is preserved");
    check(isnan(proto_models_values_output_get_float_value(output)) && isinf(proto_models_values_output_get_double_value(output)), "nonfinite floats");
    check(equal(proto_models_values_output_get_renamed(output), "hello"), "renamed member");
    check(proto_models_values_output_get_when(output).seconds == -1 && proto_models_values_output_get_when(output).nanoseconds == 750000000, "epoch timestamp");
    const proto_models_choice *choice = proto_models_values_output_get_choice(output);
    check(choice && choice->tag == proto_models_choice_tag_text && equal(choice->value.text, "ok"), "union ignores __type");
    proto_operations_echo_result_free(&result); proto_client_free(client);

    client = client_for("error");
    proto_models_values_input empty = {0};
    check(proto_operations_echo(client, &empty, NULL, NULL, &result, &error) == proto_service_error, "modeled failure");
    check(equal(result.base.name, "Problem") && equal(result.base.message, "bad") && equal(result.base.request_id, "request-42") && result.base.response.status == 409, "AWS error metadata");
    check(proto_error_as_problem(&result.base) != NULL, "typed AWS error");
    proto_operations_echo_result_free(&result); proto_client_free(client);

    client = client_for("empty");
    proto_operations_empty_result nothing = {0};
    good(proto_operations_empty(client, NULL, NULL, &nothing, &error), &error, "empty operation");
    proto_operations_empty_result_free(&nothing); proto_client_free(client);
    puts("C AWS JSON protocol passed");
    return 0;
}
