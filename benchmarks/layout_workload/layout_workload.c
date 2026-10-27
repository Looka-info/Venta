#include <stdio.h>
#include <stdint.h>

static int64_t classify(int64_t x) {
    if (x % 2 == 0) {
        return x / 2;
    } else if (x % 3 == 0) {
        return x / 3 + 1;
    } else if (x % 5 == 0) {
        return x / 5 + 2;
    } else {
        return x + 7;
    }
}

int main(void) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < 10000000) {
        sum = (sum + classify(i)) % 1000000007;
        i += 1;
    }
    printf("%ld\n", sum);
    return 0;
}
