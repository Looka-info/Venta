#include <stdio.h>
#include <stdint.h>

static int64_t compute(int64_t factor_a, int64_t factor_b, int64_t iters) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < iters) {
        int64_t inv1 = factor_a * 17 + 31;
        int64_t inv2 = factor_b * 13 - 19;
        int64_t inv3 = (inv1 ^ inv2) + (factor_a * 5);
        int64_t inv4 = (inv3 & 65535) * 7;
        sum = (sum + i) ^ inv4;
        i = i + 1;
    }
    return sum;
}

int main(void) {
    int64_t res = compute(100, 200, 100000000);
    printf("%ld\n", res);
    return 0;
}
