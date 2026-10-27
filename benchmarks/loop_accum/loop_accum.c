#include <stdio.h>
#include <stdint.h>

int main(void) {
    int64_t a = 1;
    int64_t b = 2;
    int64_t c = 3;
    int64_t d = 4;
    for (int64_t i = 0; i < 20000000; i++) {
        a = (a + b) ^ (c + d);
        b = (b + 3) ^ a;
        c = (c + 5) ^ b;
        d = (d + 7) ^ c;
    }
    int64_t total = a + b + c + d;
    printf("%ld\n", total);
    return 0;
}
