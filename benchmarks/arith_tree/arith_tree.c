#include <stdio.h>
#include <stdint.h>

static int64_t eval_tree(int64_t x, int64_t y) {
    int64_t a = (x + y) * (x - y) + (x * 3) - (y * 5);
    int64_t b = (a + x) * 2 - (a - y) * 3 + (x + y);
    int64_t c = (a + b) + (a * 2) - (b * 2) + 7;
    return c;
}

int main(void) {
    int64_t sum = 0;
    for (int64_t i = 0; i < 2000000; i++) {
        sum += eval_tree(i % 100, (i + 3) % 100);
    }
    printf("%ld\n", sum);
    return 0;
}
