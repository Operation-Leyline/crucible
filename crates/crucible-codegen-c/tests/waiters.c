#include "jobs_client/client.h"
#define PREFIX jobs
#include "echo.h"

#define VIEW(text) jobs_string_view(text)
typedef jobs_status (*waiter_fn)(jobs_client *, const jobs_models_input *, double, const jobs_waiter_options *, jobs_cancelled_fn, void *,
    jobs_operations_read_result *, jobs_waiter_report *, jobs_error *);
static jobs_client *client_for(const char *mode, const char *run) {
    static jobs_header headers[2];
    headers[0] = (jobs_header){VIEW("x-mode"), VIEW(mode)}; headers[1] = (jobs_header){VIEW("x-run"), VIEW(run)};
    jobs_client_config config = jobs_client_defaults();
    config.endpoint = VIEW(environment("FEATURES_URL")); config.max_attempts = 1;
    config.headers = (jobs_headers){headers, 2};
    jobs_client *client = NULL; jobs_error error = {0};
    good(jobs_client_new(&config, &client, &error), &error, "client_new");
    return client;
}
static bool cancelled(void *user) { (void)user; return true; }

int main(void) {
    jobs_string names[] = {VIEW("a"), VIEW("b")};
    jobs_models_names list = {2, names};
    jobs_models_input input = {0};
    jobs_models_input_set_names(&input, &list);
    jobs_waiter_options fast = {true, true, 0, 0};
    jobs_operations_read_result result = {0}; jobs_waiter_report report = {0}; jobs_error error = {0};

    jobs_client *client = client_for("ready", "ready");
    good(jobs_operations_read_wait_until_ready(client, &input, 2, &fast, NULL, NULL, &result, &report, &error), &error, "ready");
    check(result.value && equal(jobs_models_output_get_status(result.value), "READY") && report.attempts == 3, "retry through a modeled error to success");
    jobs_operations_read_result_free(&result); jobs_client_free(client);

    client = client_for("gone", "gone");
    good(jobs_operations_read_wait_until_deleted(client, &input, 2, &fast, NULL, NULL, &result, &report, &error), &error, "deleted");
    check(!result.value && equal(result.base.name, "Missing") && result.base.response.status == 404, "error acceptor succeeds with the failure response");
    jobs_operations_read_result_free(&result); jobs_client_free(client);

    client = client_for("query", "query");
    waiter_fn queries[] = {jobs_operations_read_wait_until_all_ready, jobs_operations_read_wait_until_any_ready, jobs_operations_read_wait_until_filtered_ready,
                           jobs_operations_read_wait_until_counts_match, jobs_operations_read_wait_until_updated, jobs_operations_read_wait_until_functions};
    const char *query_names[] = {"AllReady", "AnyReady", "FilteredReady", "CountsMatch", "Updated", "Functions"};
    for (size_t i = 0; i < sizeof(queries) / sizeof(queries[0]); ++i) {
        good(queries[i](client, &input, 2, &fast, NULL, NULL, &result, &report, &error), &error, query_names[i]);
        check(report.attempts == 1, query_names[i]);
        jobs_operations_read_result_free(&result);
    }
    jobs_models_names one = {1, names};
    jobs_models_input_set_names(&input, &one);
    check(jobs_operations_read_wait_until_counts_match(client, &input, 0.2, &fast, NULL, NULL, &result, &report, &error) == jobs_deadline && report.attempts > 1, "inputOutput mismatch keeps polling");
    jobs_operations_read_result_free(&result);
    jobs_models_input_set_names(&input, &list);
    jobs_client_free(client);

    client = client_for("failed", "failed");
    check(jobs_operations_read_wait_until_ready(client, &input, 2, &fast, NULL, NULL, &result, &report, &error) == jobs_waiter_failed, "failure acceptor");
    check(report.attempts == 1 && contains(report.message, "job rejected") && report.cause == jobs_ok, "failure message and attempts");
    check(result.value && equal(jobs_models_output_get_status(result.value), "FAILED"), "failure keeps the response");
    jobs_operations_read_result_free(&result);
    check(jobs_operations_read_wait_until_ready(client, &input, 2, NULL, NULL, NULL, NULL, &report, &error) == jobs_invalid, "result is required");
    jobs_waiter_options reversed = {true, true, 2, 1};
    check(jobs_operations_read_wait_until_ready(client, &input, 2, &reversed, NULL, NULL, &result, &report, &error) == jobs_invalid, "ordered delays");
    jobs_operations_read_result_free(&result);
    check(jobs_operations_read_wait_until_ready(client, &input, 0, NULL, NULL, NULL, &result, &report, &error) == jobs_invalid, "positive timeout");
    jobs_operations_read_result_free(&result);
    jobs_client_free(client);

    client = client_for("pending", "pending");
    check(jobs_operations_read_wait_until_ready(client, &input, 0.05, NULL, NULL, NULL, &result, &report, &error) == jobs_deadline && report.attempts >= 1, "waiter deadline");
    jobs_operations_read_result_free(&result);
    check(jobs_operations_read_wait_until_ready(client, &input, 1, NULL, cancelled, NULL, &result, &report, &error) == jobs_cancelled, "waiter cancellation");
    jobs_operations_read_result_free(&result);
    jobs_client_free(client);

    client = client_for("blocked", "blocked");
    check(jobs_operations_read_wait_until_ready(client, &input, 0.05, NULL, NULL, NULL, &result, &report, &error) == jobs_deadline, "deadline interrupts a blocked attempt");
    jobs_operations_read_result_free(&result);
    jobs_client_free(client);
    puts("C waiters passed");
    return 0;
}
