#include <stdio.h>
#include <stdint.h>

typedef struct {
    int64_t x;
    int64_t y;
    int64_t z;
} Vec3;

static inline int64_t process(const Vec3 *v) {
    return v->x + v->y + v->z;
}

static int64_t compute_sroa(int64_t iters) {
    int64_t sum = 0;
    int64_t i = 0;
    while (i < iters) {
        Vec3 v = {
            .x = i,
            .y = i + 1,
            .z = i + 2,
        };
        sum += process(&v);
        i += 1;
    }
    return sum;
}

int main(void) {
    int64_t res = compute_sroa(20000000);
    printf("%ld\n", res);
    return 0;
}
