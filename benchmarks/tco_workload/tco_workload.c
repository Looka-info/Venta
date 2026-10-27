#include <stdio.h>
#include <stdint.h>

static int64_t tail_sum(int64_t n, int64_t acc) {
    if (n == 0) {
        return acc;
    }
    return tail_sum(n - 1, (acc + n) % 1000000007);
}

int main(void) {
    int64_t total = 0;
    int64_t i = 0;
    while (i < 10000) {
        total = (total + tail_sum(1000, 0)) % 1000000007;
        i += 1;
    }
    printf("%ld\n", total);
    return 0;
}
