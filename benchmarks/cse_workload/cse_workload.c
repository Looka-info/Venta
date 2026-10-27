#include <stdio.h>
#include <stdint.h>

__attribute__((noinline))
int64_t cse_kernel(int64_t x, int64_t y) {
    int64_t t1 = x * 13 + y * 17;
    int64_t t2 = y * 17 + x * 13;
    int64_t t3 = (x * 13 + y * 17) ^ (x * 13);
    int64_t t4 = t1 + t2 + t3;
    int64_t t5 = (x * 13 + y * 17) * 3;
    int64_t t6 = (y * 17 + x * 13) - 5;
    return t4 + t5 + t6;
}

int main(void) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < 10000000) {
        sum += cse_kernel(i % 50, (i + 7) % 50);
        i++;
    }
    printf("%ld\n", (long)sum);
    return 0;
}
