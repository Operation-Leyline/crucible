#include "features.h"
#include <openssl/rand.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>

typedef crucible_generated_status status;
typedef crucible_generated_string string;
typedef crucible_generated_error error;
typedef crucible_generated_arena arena;
#define OK crucible_generated_ok
#define INVALID crucible_generated_invalid
#define OOM crucible_generated_out_of_memory
#define TRY(call) do { status code_ = (call); if (code_) return code_; } while (0)
#define VIEW(text) crucible_generated_string_view(text)
static const yyjson_val *get(const yyjson_val *value, const char *key) { return yyjson_obj_get((yyjson_val *)value, key); }
static string text(const yyjson_val *value) { return (string){yyjson_get_str((yyjson_val *)value), yyjson_get_len((yyjson_val *)value)}; }
static bool named(const yyjson_val *value, const char *name) { return yyjson_is_str((yyjson_val *)value) && crucible_generated_string_equal(text(value), VIEW(name)); }

static status delay_value(const yyjson_val *value, double *out, error *issue) {
    int64_t seconds;
    if (crucible_generated_json_integer(value, 0, INT64_MAX / 2, &seconds, NULL)) return crucible_generated_fail(issue, INVALID, "Invalid waiter delay");
    *out = (double)seconds; return OK;
}
static double fraction(void) {
    unsigned char random[4];
    if (RAND_bytes(random, 4) != 1) return 0.5;
    uint32_t bits = ((uint32_t)random[0] << 24) | ((uint32_t)random[1] << 16) | ((uint32_t)random[2] << 8) | random[3];
    return (double)bits / 4294967296.0;
}
typedef enum { state_retry, state_success, state_failure } waiter_state;
/* The first matching acceptor decides; an unmatched failure is a failure state. */
static status evaluate(crucible_generated_query *query, const yyjson_val *description, yyjson_mut_val *input, yyjson_mut_val *output,
    bool failed, const crucible_generated_result *result, waiter_state *state, yyjson_mut_val **message) {
    *message = NULL;
    size_t index, count; yyjson_val *acceptor;
    yyjson_arr_foreach((yyjson_val *)get(description, "acceptors"), index, count, acceptor) {
        const yyjson_val *matcher = get(acceptor, "matcher"), *kind = get(matcher, "kind");
        bool matches = false;
        if (named(kind, "success")) matches = !failed == yyjson_get_bool((yyjson_val *)get(matcher, "expected"));
        else if (named(kind, "errorType")) {
            if (failed && result->name.length) {
                string expected = text(get(matcher, "name")), last = expected;
                for (size_t i = expected.length; i > 0; --i) if (expected.data[i - 1] == '#') { last = (string){expected.data + i, expected.length - i}; break; }
                matches = crucible_generated_string_equal(result->name, expected) || crucible_generated_string_equal(result->name, last);
            }
        } else if (!failed) {
            yyjson_mut_val *source = output;
            if (named(kind, "inputOutput")) {
                source = yyjson_mut_obj(query->doc);
                yyjson_mut_val *in = input ? yyjson_mut_val_mut_copy(query->doc, input) : yyjson_mut_null(query->doc);
                yyjson_mut_val *out = output ? yyjson_mut_val_mut_copy(query->doc, output) : yyjson_mut_null(query->doc);
                if (!source || !in || !out || !yyjson_mut_obj_add_val(query->doc, source, "input", in) || !yyjson_mut_obj_add_val(query->doc, source, "output", out)) return crucible_generated_fail(query->issue, OOM, "JSON allocation failed");
            }
            yyjson_mut_val *value;
            TRY(crucible_generated_query_eval(query, get(get(matcher, "path"), "expression"), source, &value));
            string expected = text(get(matcher, "expected"));
            const yyjson_val *comparator = get(matcher, "comparator");
            if (named(comparator, "stringEquals")) matches = value && yyjson_mut_is_str(value) && crucible_generated_string_equal((string){yyjson_mut_get_str(value), yyjson_mut_get_len(value)}, expected);
            else if (named(comparator, "booleanEquals")) matches = value && yyjson_mut_is_bool(value) && yyjson_mut_get_bool(value) == crucible_generated_string_equal(expected, VIEW("true"));
            else if (named(comparator, "allStringEquals") || named(comparator, "anyStringEquals")) {
                if (value && yyjson_mut_is_arr(value)) {
                    bool all = yyjson_mut_arr_size(value) > 0, any = false;
                    size_t ai, ac; yyjson_mut_val *item;
                    yyjson_mut_arr_foreach(value, ai, ac, item) {
                        bool equal = yyjson_mut_is_str(item) && crucible_generated_string_equal((string){yyjson_mut_get_str(item), yyjson_mut_get_len(item)}, expected);
                        all = all && equal; any = any || equal;
                    }
                    matches = named(comparator, "allStringEquals") ? all : any;
                }
            }
        }
        if (!matches) continue;
        const yyjson_val *query_message = get(acceptor, "message");
        if (yyjson_is_obj((yyjson_val *)query_message)) TRY(crucible_generated_query_eval(query, get(query_message, "expression"), output, message));
        const yyjson_val *target = get(acceptor, "state");
        *state = named(target, "success") ? state_success : named(target, "failure") ? state_failure : state_retry;
        return OK;
    }
    *state = failed ? state_failure : state_retry;
    return OK;
}
status crucible_generated_client_wait(crucible_generated_client *client, string operation, string waiter, const void *input, double timeout,
    const crucible_generated_waiter_options *options, crucible_generated_cancelled_fn cancelled, void *cancel_user,
    crucible_generated_result *result, crucible_generated_waiter_report *report, error *issue) {
    if (!client || !result || (!operation.data && operation.length) || (!waiter.data && waiter.length)) return crucible_generated_fail(issue, INVALID, "Client, operation, waiter and result are required");
    *result = (crucible_generated_result){0};
    crucible_generated_waiter_report ignored;
    if (!report) report = &ignored;
    *report = (crucible_generated_waiter_report){0};
    const yyjson_val *op = yyjson_obj_getn((yyjson_val *)get(client->schema, "operations"), operation.data, operation.length);
    const yyjson_val *description = yyjson_obj_getn((yyjson_val *)get(op, "waiters"), waiter.data, waiter.length);
    if (!yyjson_is_obj((yyjson_val *)description)) return crucible_generated_fail(issue, INVALID, "Unknown waiter");
    double minimum, maximum;
    TRY(delay_value(get(description, "minDelay"), &minimum, issue));
    TRY(delay_value(get(description, "maxDelay"), &maximum, issue));
    if (options && options->has_min_delay) minimum = options->min_delay;
    if (options && options->has_max_delay) maximum = options->max_delay;
    if (!isfinite(timeout) || timeout <= 0 || !isfinite(minimum) || !isfinite(maximum) || minimum < 0 || maximum < minimum) return crucible_generated_fail(issue, INVALID, "Waiter deadline and ordered delays must be valid");
    crucible_generated_scope deadline;
    TRY(crucible_generated_scope_begin(timeout, cancelled, cancel_user, &deadline, issue));
    arena *storage = crucible_generated_arena_new();
    if (!storage) return crucible_generated_fail(issue, OOM, "Waiter allocation failed");
    const yyjson_val *encoded;
    crucible_generated_query query;
    yyjson_mut_val *input_model = NULL;
    status code = crucible_generated_encode_input(storage, op, input, &encoded, issue);
    if (!code) code = crucible_generated_query_begin(storage, &query, issue);
    if (!code) code = crucible_generated_query_model(&query, client->schema, get(op, "input"), encoded, &input_model);
    crucible_generated_result last = {0};
    for (;;) {
        if (code) break;
        status stopped = crucible_generated_scope_check(&deadline, NULL);
        if (stopped) { code = crucible_generated_fail(issue, stopped, stopped == crucible_generated_cancelled ? "Waiter cancelled" : "Waiter deadline exceeded"); break; }
        ++report->attempts;
        /* Each attempt is bounded by the client timeout and the waiter's remaining time. */
        crucible_generated_scope attempt;
        code = crucible_generated_scope_begin(client->config.timeout, cancelled, cancel_user, &attempt, issue);
        if (code) break;
        attempt.deadline = fmin(attempt.deadline, deadline.deadline);
        crucible_generated_result_free(&last);
        arena *prepared = crucible_generated_arena_new();
        if (!prepared) { code = crucible_generated_fail(issue, OOM, "Waiter allocation failed"); break; }
        const yyjson_val *wire = NULL;
        error call_issue = {0};
        status call = crucible_generated_execute(client, prepared, operation, op, encoded, &attempt, &last, &wire, &call_issue);
        crucible_generated_arena_free(prepared);
        stopped = crucible_generated_scope_check(&deadline, NULL);
        if (stopped) { code = crucible_generated_fail(issue, stopped, stopped == crucible_generated_cancelled ? "Waiter cancelled" : "Waiter deadline exceeded"); report->cause = call; break; }
        /* Failures before a response (bindings, endpoints, OOM) end the wait unchanged. */
        if (call == OOM || !last.arena) { if (issue) *issue = call_issue; code = call; break; }
        crucible_generated_query attempt_query;
        yyjson_mut_val *output = NULL, *message = NULL;
        waiter_state state = state_retry;
        code = crucible_generated_query_begin(last.arena, &attempt_query, issue);
        if (!code && !call) code = crucible_generated_query_model(&attempt_query, client->schema, get(op, "output"), wire, &output);
        if (!code) code = evaluate(&attempt_query, description, input_model, output, call != OK, &last, &state, &message);
        if (code) break;
        report->cause = call;
        /* A success acceptor may match a modeled error, such as a deleted resource. */
        if (state == state_success) break;
        if (state == state_failure) {
            if (message) {
                size_t length = 0; char *written = yyjson_mut_val_write(message, 0, &length);
                if (!written) { code = crucible_generated_fail(issue, OOM, "JSON allocation failed"); break; }
                code = crucible_generated_arena_copy(last.arena, (string){written, length}, &report->message, issue);
                free(written);
                if (code) break;
            }
            code = crucible_generated_fail(issue, crucible_generated_waiter_failed, call ? call_issue.message : "Waiter reached a failure state");
            break;
        }
        double ceiling = fmin(maximum, minimum * pow(2, fmin((double)report->attempts - 1, 62)));
        double delay = minimum + fraction() * (ceiling - minimum);
        double remaining = crucible_generated_scope_remaining(&deadline);
        if (remaining <= minimum) delay = remaining > 0 ? remaining : 0;
        else if (remaining - delay <= minimum) delay = remaining - minimum;
        code = crucible_generated_scope_wait(&deadline, delay, issue);
        if (code) { if (code == crucible_generated_deadline) code = crucible_generated_fail(issue, code, "Waiter deadline exceeded"); break; }
    }
    *result = last;
    crucible_generated_arena_free(storage);
    return code;
}
