#include <stdio.h>
#include <stdint.h>

static int64_t compute_pressure(int64_t x) {
    int64_t v1 = x + 1;
    int64_t v2 = x + 2;
    int64_t v3 = x + 3;
    int64_t v4 = x + 4;
    int64_t v5 = x + 5;
    int64_t v6 = x + 6;
    int64_t v7 = x + 7;
    int64_t v8 = x + 8;
    int64_t v9 = x + 9;
    int64_t v10 = x + 10;
    int64_t res = (v1 * v2) - (v3 * v4) + (v5 * v6) - (v7 * v8) + (v9 * v10)
        + (v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10);
    return res;
}

int main(void) {
    int64_t sum = 0;
    for (int64_t i = 0; i < 5000000; i++) {
        sum += compute_pressure(i % 50);
    }
    printf("%ld\n", sum);
    return 0;
}
