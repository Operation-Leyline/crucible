#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif
#include <R.h>
#include <Rinternals.h>
#include <R_ext/Rdynload.h>
#include <R_ext/Visibility.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#endif

/* Deadline accounting must not follow wall-clock corrections. */
static SEXP crucible_now(void) {
#ifdef _WIN32
    LARGE_INTEGER counter, frequency;
    if (!QueryPerformanceFrequency(&frequency) || !QueryPerformanceCounter(&counter))
        Rf_error("Cannot read monotonic clock");
    return Rf_ScalarReal((double) counter.QuadPart / (double) frequency.QuadPart);
#else
    struct timespec value;
    if (clock_gettime(CLOCK_MONOTONIC, &value) != 0)
        Rf_error("Cannot read monotonic clock");
    return Rf_ScalarReal((double) value.tv_sec + (double) value.tv_nsec / 1e9);
#endif
}

static const R_CallMethodDef methods[] = {
    {"crucible_now", (DL_FUNC) &crucible_now, 0},
    {NULL, NULL, 0}
};

void attribute_visible R_init_CRUCIBLE_PACKAGE(DllInfo *dll) {
    R_registerRoutines(dll, NULL, methods, NULL, NULL);
    R_useDynamicSymbols(dll, FALSE);
    R_forceSymbols(dll, TRUE);
}
