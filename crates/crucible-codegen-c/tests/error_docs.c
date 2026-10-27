#include "docs_client/client.h"
#include <yyjson.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(bool condition, const char *label) {
    if (!condition) { fprintf(stderr, "Error documentation: %s\n", label); exit(1); }
}
int main(void) {
    yyjson_doc *oracle = yyjson_read_file(getenv("ERROR_DOCS_CASES"), 0, NULL, NULL);
    check(oracle != NULL, "read shared oracle");
    yyjson_val *cases = yyjson_obj_get(yyjson_doc_get_root(oracle), "cases"), *test;
    size_t index, count;
    yyjson_arr_foreach(cases, index, count, test) {
        const char *label = yyjson_get_str(yyjson_obj_get(test, "name"));
        docs_client_config config = docs_client_defaults();
        config.endpoint = docs_string_view(getenv("ERROR_DOCS_URL"));
        config.max_attempts = 1;
        docs_client *client = NULL;
        docs_error issue = {0};
        check(docs_client_new(&config, &client, &issue) == docs_ok, label);
        docs_operations_inspect_result result = {0};
        check(docs_operations_inspect(client, NULL, NULL, &result, &issue) == docs_service_error, label);
        /* Both code and reason URLs must remain valid after the schema arena dies. */
        docs_client_free(client);
        yyjson_val *expected = yyjson_obj_get(test, "docUrl");
        if (yyjson_is_null(expected)) check(result.base.doc_url.data == NULL, label);
        else check(docs_string_equal(result.base.doc_url, (docs_string){yyjson_get_str(expected), yyjson_get_len(expected)}), label);
        yyjson_val *typed = yyjson_obj_get(test, "typed");
        if (yyjson_is_bool(typed)) check((result.base.error_shape != NULL) == yyjson_get_bool(typed), label);
        if (strcmp(label, "default-known") == 0) {
            const docs_models_default_reason *model = docs_error_as_default_reason(&result.base);
            check(model && docs_string_equal(docs_models_default_reason_get_detail(model), docs_string_view("access/needed")), label);
        }
        if (strcmp(label, "default-empty") == 0) {
            const docs_models_default_empty_reason *model = docs_error_as_default_empty_reason(&result.base);
            check(model && docs_models_default_empty_reason_has_detail(model) && docs_models_default_empty_reason_get_detail(model).length == 0, label);
        }
        if (strcmp(label, "mapped-member") == 0) {
            const docs_models_mapped_reason_error *model = docs_error_as_mapped_reason_error(&result.base);
            check(model && docs_string_equal(docs_models_mapped_reason_error_get_failure_reason(model), docs_string_view("access/needed")), label);
        }
        if (strcmp(label, "document-empty") == 0 || strcmp(label, "document-whitespace") == 0 || strcmp(label, "document-null") == 0) {
            const docs_models_document_payload_error *model = docs_error_as_document_payload_error(&result.base);
            check(model && model->data.state == (strcmp(label, "document-null") == 0 ? docs_null : docs_absent), label);
        }
        docs_operations_inspect_result_free(&result);
    }
    yyjson_doc_free(oracle);
    puts("C installed error-documentation HTTP and result lifetime passed");
    return 0;
}
