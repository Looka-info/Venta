#include <stdint.h>
#include <stdio.h>

static int64_t dot4(int64_t acc) {
    int64_t total = acc;
    int64_t k = 0;
    while (k < 4) {
        total = total + (k * 3 + 1);
        k = k + 1;
    }
    return total;
}

int main(void) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < 10000000) {
        sum = dot4(sum);
        i = i + 1;
    }
    printf("%ld\n", (long)sum);
    return 0;
}
