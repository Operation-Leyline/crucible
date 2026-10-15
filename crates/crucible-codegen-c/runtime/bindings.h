#ifndef CRUCIBLE_GENERATED_BINDINGS_H
#define CRUCIBLE_GENERATED_BINDINGS_H
#include "client_runtime.h"
#include "internal.h"

crucible_generated_status crucible_generated_json_own(crucible_generated_arena *, const yyjson_mut_val *, const yyjson_val **, crucible_generated_error *);
crucible_generated_status crucible_generated_binding_prepare(crucible_generated_arena *, const yyjson_val *, crucible_generated_string, const yyjson_val *, const yyjson_val *, crucible_generated_string, crucible_generated_headers, crucible_generated_request *, crucible_generated_error *);
crucible_generated_status crucible_generated_binding_success(crucible_generated_arena *, const yyjson_val *, const yyjson_val *, const crucible_generated_response *, const yyjson_val **, crucible_generated_error *);
crucible_generated_status crucible_generated_binding_failure(crucible_generated_arena *, const yyjson_val *, const yyjson_val *, crucible_generated_result *, crucible_generated_error *);
crucible_generated_status crucible_generated_binding_token(crucible_generated_arena *, const yyjson_val *, const yyjson_val *, const yyjson_val **, crucible_generated_error *);
#endif
