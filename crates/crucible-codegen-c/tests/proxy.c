#ifdef __linux__
#define _POSIX_C_SOURCE 200809L
#endif
#include "weather_client/client.h"
#include <stdio.h>
#ifdef __linux__
#include <pthread.h>
#include <signal.h>
#endif
int main(void) {
    weather_client_config settings = weather_client_defaults(); weather_client *client = NULL;
    weather_error error = {0}; weather_operations_ping_result result = {0};
    settings.endpoint = weather_string_view("https://unreachable.invalid"); settings.max_attempts = 1;
    if (weather_client_new(&settings, &client, &error) != weather_ok) { fprintf(stderr, "%s\n", error.message); return 1; }
#ifdef __linux__
    sigset_t before, after;
    (void)pthread_sigmask(SIG_SETMASK, NULL, &before);
#endif
    weather_status status = weather_operations_ping(client, NULL, NULL, &result, &error);
    weather_operations_ping_result_free(&result); weather_client_free(client);
#ifdef __linux__
    (void)pthread_sigmask(SIG_SETMASK, NULL, &after);
    if (sigismember(&before, SIGPIPE) != sigismember(&after, SIGPIPE)) { fputs("SIGPIPE mask changed\n", stderr); return 1; }
#endif
    if (status != weather_network) { fprintf(stderr, "CONNECT response misclassified: %d %s\n", (int)status, error.message); return 1; }
    puts("C proxy tunnel headers cannot become a service response"); return 0;
}
