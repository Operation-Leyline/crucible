/* Minimal portable threads and atomics for concurrency tests. */
#ifndef THREADS_TEST_H
#define THREADS_TEST_H
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
typedef HANDLE worker;
typedef volatile LONG atomic_flag_value;
#define ATOMIC_GET(target) InterlockedCompareExchange(&(target), 0, 0)
#define ATOMIC_SET(target, value) InterlockedExchange(&(target), (value))
#define ATOMIC_ADD(target) InterlockedIncrement(&(target))
static void pause_ms(unsigned milliseconds) { Sleep(milliseconds); }
#else
#include <pthread.h>
#include <time.h>
typedef pthread_t worker;
typedef long atomic_flag_value;
#define ATOMIC_GET(target) __atomic_load_n(&(target), __ATOMIC_SEQ_CST)
#define ATOMIC_SET(target, value) __atomic_store_n(&(target), (value), __ATOMIC_SEQ_CST)
#define ATOMIC_ADD(target) __atomic_add_fetch(&(target), 1, __ATOMIC_SEQ_CST)
static void pause_ms(unsigned milliseconds) {
    struct timespec delay = {(time_t)(milliseconds / 1000), (long)(milliseconds % 1000) * 1000000L};
    (void)nanosleep(&delay, NULL);
}
#endif
typedef struct { void (*run)(void *); void *argument; } job;
#ifdef _WIN32
static DWORD WINAPI trampoline(LPVOID value) { job *task = value; task->run(task->argument); return 0; }
static bool spawn(worker *thread, job *task) { *thread = CreateThread(NULL, 0, trampoline, task, 0, NULL); return *thread != NULL; }
static void join(worker thread) { (void)WaitForSingleObject(thread, INFINITE); (void)CloseHandle(thread); }
#else
static void *trampoline(void *value) { job *task = value; task->run(task->argument); return NULL; }
static bool spawn(worker *thread, job *task) { return pthread_create(thread, NULL, trampoline, task) == 0; }
static void join(worker thread) { (void)pthread_join(thread, NULL); }
#endif
#endif
