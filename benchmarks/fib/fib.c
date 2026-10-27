#include <stdio.h>
#include <stdint.h>

__attribute__((noinline))
int64_t fib(int64_t n) {
    if (n <= 1) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

int main() {
    int64_t res = fib(38);
    printf("%ld\n", res);
    return 0;
}
