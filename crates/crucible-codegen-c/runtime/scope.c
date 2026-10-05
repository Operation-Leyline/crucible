#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif
#include "http.h"
#include "internal.h"
#include <math.h>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#else
#include <time.h>
#endif

static double now(void) {
#ifdef _WIN32
    LARGE_INTEGER ticks, frequency;
    QueryPerformanceCounter(&ticks);
    QueryPerformanceFrequency(&frequency);
    return (double)ticks.QuadPart / (double)frequency.QuadPart;
#else
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0) return NAN;
    return (double)value.tv_sec + (double)value.tv_nsec / 1e9;
#endif
}
double crucible_generated_scope_remaining(const crucible_generated_scope *scope) { return scope->deadline - now(); }
crucible_generated_status crucible_generated_scope_begin(double timeout, crucible_generated_cancelled_fn cancelled, void *user,
    crucible_generated_scope *scope, crucible_generated_error *error) {
    if (!scope || !isfinite(timeout) || timeout <= 0) return crucible_generated_fail(error, crucible_generated_invalid, "Timeout must be finite and positive");
    *scope = (crucible_generated_scope){ now() + timeout, cancelled, user };
    if (!isfinite(scope->deadline)) return crucible_generated_fail(error, crucible_generated_invalid, "Deadline is out of range");
    return crucible_generated_scope_check(scope, error);
}
crucible_generated_status crucible_generated_scope_check(const crucible_generated_scope *scope, crucible_generated_error *error) {
    if (!scope) return crucible_generated_fail(error, crucible_generated_invalid, "Missing call scope");
    if (scope->cancelled && scope->cancelled(scope->user)) return crucible_generated_fail(error, crucible_generated_cancelled, "Call cancelled");
    if (!(crucible_generated_scope_remaining(scope) > 0)) return crucible_generated_fail(error, crucible_generated_deadline, "Call deadline exceeded");
    return crucible_generated_ok;
}
crucible_generated_status crucible_generated_scope_wait(const crucible_generated_scope *scope, double seconds, crucible_generated_error *error) {
    if (!isfinite(seconds) || seconds < 0) return crucible_generated_fail(error, crucible_generated_invalid, "Invalid wait duration");
    const double end = now() + seconds;
    for (;;) {
        crucible_generated_status status = crucible_generated_scope_check(scope, error);
        if (status) return status;
        double remaining = end - now();
        if (remaining <= 0) return crucible_generated_ok;
        remaining = fmin(0.01, fmin(remaining, crucible_generated_scope_remaining(scope)));
        if (!(remaining > 0)) continue;
#ifdef _WIN32
        Sleep((DWORD)ceil(remaining * 1000));
#else
        struct timespec delay = {0, (long)(remaining * 1e9)};
        (void)nanosleep(&delay, NULL);
#endif
    }
}
