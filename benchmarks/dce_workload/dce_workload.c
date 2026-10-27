#include <stdio.h>
#include <stdint.h>

__attribute__((unused))
static int64_t dead_helper_1(int64_t x) {
    return x * 3 + 42;
}

__attribute__((unused))
static int64_t dead_helper_2(int64_t x) {
    return dead_helper_1(x) * 2;
}

static int64_t compute(int64_t n) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < n) {
        int64_t dead_val = (i * 17 + 4) / 3;
        int64_t dead_val2 = dead_val * dead_val + 1;
        (void)dead_val2;
        sum = (sum + i) % 1000000007;
        i += 1;
    }
    return sum;
}

int main(void) {
    int64_t total = 0;
    int64_t i = 0;
    while (i < 1000) {
        total = (total + compute(10000)) % 1000000007;
        i += 1;
    }
    printf("%ld\n", total);
    return 0;
}
