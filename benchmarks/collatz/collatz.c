#include <stdio.h>
#include <stdint.h>

__attribute__((noinline))
int64_t collatz_len(int64_t n) {
    int64_t steps = 0;
    while (n > 1) {
        if (n % 2 == 0) {
            n = n / 2;
        } else {
            n = 3 * n + 1;
        }
        steps++;
    }
    return steps;
}

int main() {
    int64_t total_steps = 0;
    for (int64_t i = 1; i <= 500000; i++) {
        total_steps += collatz_len(i);
    }
    printf("%ld\n", total_steps);
    return 0;
}
