#include <stdio.h>
#include <stdint.h>

int64_t add3(int64_t a, int64_t b, int64_t c) {
    return a + b + c;
}

int64_t scale_add(int64_t x, int64_t factor, int64_t bias) {
    return x * factor + bias;
}

int64_t leaf_work(int64_t val) {
    int64_t s1 = add3(val, 15, 27);
    int64_t s2 = scale_add(s1, 3, 7);
    return s2;
}

int main(void) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < 10000000) {
        sum += leaf_work(i % 500) % 1000;
        i++;
    }
    printf("%ld\n", sum);
    return 0;
}
