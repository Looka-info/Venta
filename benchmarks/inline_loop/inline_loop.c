#include <stdio.h>
#include <stdint.h>

int64_t inner_accum(int64_t base, int64_t limit) {
    int64_t sum = 0;
    int64_t j = 0;
    while (j < limit) {
        sum += base * 2 + j;
        j++;
    }
    return sum;
}

int main(void) {
    int64_t total = 0;
    int64_t i = 0;
    while (i < 100000) {
        total += inner_accum(i % 100, 20) % 5000;
        i++;
    }
    printf("%ld\n", total);
    return 0;
}
