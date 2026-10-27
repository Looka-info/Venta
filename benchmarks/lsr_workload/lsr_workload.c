#include <stdio.h>
#include <stdint.h>

static int64_t compute_lsr(int64_t scale, int64_t iters) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < iters) {
        int64_t term = i * 9;
        sum += term;
        i += 1;
    }
    return sum;
}

int main(void) {
    int64_t res = compute_lsr(9, 50000000);
    printf("%ld\n", res);
    return 0;
}
