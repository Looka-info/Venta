#include <stdio.h>
#include <stdint.h>

typedef struct {
    int64_t x;
    int64_t y;
    int64_t z;
} Point;

__attribute__((noinline))
int64_t manhattan(const Point *p) {
    int64_t x = p->x < 0 ? -p->x : p->x;
    int64_t y = p->y < 0 ? -p->y : p->y;
    int64_t z = p->z < 0 ? -p->z : p->z;
    return x + y + z;
}

int main() {
    int64_t total = 0;
    for (int64_t i = 0; i < 1000000; i++) {
        Point p = {
            .x = i % 100 - 50,
            .y = (i * 3) % 100 - 50,
            .z = (i * 7) % 100 - 50,
        };
        total += manhattan(&p);
    }
    printf("%ld\n", total);
    return 0;
}
