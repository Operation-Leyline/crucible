#include "weather_client/client.hpp"
#include <iostream>
#ifdef __linux__
#include <pthread.h>
#include <signal.h>
#endif
int main() {
    crucible_generated::runtime::ClientConfig config;
    config.endpoint = "https://unreachable.invalid";
    config.max_attempts = 1;
    crucible_generated::Client client(config);
#ifdef __linux__
    sigset_t before{};
    pthread_sigmask(SIG_SETMASK, nullptr, &before);
#endif
    try { (void)client.ping(); }
    catch (const crucible_generated::runtime::NetworkError&) {
#ifdef __linux__
        sigset_t after{};
        pthread_sigmask(SIG_SETMASK, nullptr, &after);
        if (sigismember(&before, SIGPIPE) != sigismember(&after, SIGPIPE)) throw std::runtime_error("Signal mask changed");
#endif
        std::cout << "C++ proxy tunnel is not a service response\n"; return 0;
    }
    throw std::runtime_error("CONNECT headers became a successful service response");
}
